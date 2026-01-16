//! File operation tools: read_file, write_file, str_replace, read_image.

use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use tracing::debug;

use crate::ui_writer::UiWriter;
use crate::utils::resolve_path_with_unicode_fallback;
use crate::utils::apply_unified_diff_to_string;
use crate::ToolCall;

use super::executor::ToolContext;

/// Bytes per token heuristic (conservative estimate for code/text mix)
const BYTES_PER_TOKEN: f32 = 3.5;

/// Maximum percentage of context window a single file read can consume
const MAX_FILE_READ_PERCENT: f32 = 0.20; // 20%

/// Estimate token count from byte size
fn estimate_tokens_from_bytes(bytes: usize) -> u32 {
    ((bytes as f32 / BYTES_PER_TOKEN) * 1.1).ceil() as u32 // 10% safety buffer
}

/// Calculate the maximum bytes we should read based on context window state.
/// Returns None if no limit needed, Some(max_bytes) if limiting required.
fn calculate_read_limit(file_bytes: usize, total_tokens: u32, used_tokens: u32) -> Option<usize> {
    let file_tokens = estimate_tokens_from_bytes(file_bytes);
    let max_tokens_for_file = (total_tokens as f32 * MAX_FILE_READ_PERCENT) as u32;
    
    // Tier 1: File is small enough (< 20% of context) - no limit
    if file_tokens < max_tokens_for_file {
        return None;
    }
    
    // Calculate available context
    let available_tokens = total_tokens.saturating_sub(used_tokens);
    let half_available = available_tokens / 2;
    
    // Tier 3: If 20% would exceed half of available, cap at half available
    let effective_max_tokens = if max_tokens_for_file > half_available {
        half_available
    } else {
        // Tier 2: Cap at 20% of total context
        max_tokens_for_file
    };
    
    // Convert tokens back to bytes
    let max_bytes = (effective_max_tokens as f32 * BYTES_PER_TOKEN / 1.1) as usize;
    
    Some(max_bytes)
}

/// Execute the `read_file` tool.
pub async fn execute_read_file<W: UiWriter>(
    tool_call: &ToolCall,
    ctx: &ToolContext<'_, W>,
) -> Result<String> {
    debug!("Processing read_file tool call (optimized with seek)");
    
    let file_path = match tool_call.args.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Ok("❌ Missing file_path argument".to_string()),
    };

    // Expand tilde (~) to home directory
    let expanded_path = shellexpand::tilde(file_path);
    // Try to resolve with Unicode space fallback (macOS uses U+202F in screenshot names)
    let resolved_path = resolve_path_with_unicode_fallback(expanded_path.as_ref());
    let path_str = resolved_path.as_ref();

    // Extract optional start and end positions
    let start_char = tool_call
        .args
        .get("start")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let end_char = tool_call
        .args
        .get("end")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);

    debug!(
        "Reading file: {}, start={:?}, end={:?}",
        path_str, start_char, end_char
    );

    // Get file metadata for size without reading content
    let path = Path::new(path_str);
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return Ok(format!("❌ Failed to read file '{}': {}", path_str, e)),
    };
    let total_file_len = metadata.len() as usize;

    // Calculate token-aware limit
    let read_limit = calculate_read_limit(
        total_file_len,
        ctx.context_total_tokens,
        ctx.context_used_tokens,
    );

    // Validate user-specified range
    let user_start = start_char.unwrap_or(0);
    let user_end = end_char.unwrap_or(total_file_len);
    
    // Clamp end position to file length
    let (user_end, end_was_clamped) = if user_end > total_file_len {
        (total_file_len, true)
    } else {
        (user_end, false)
    };
    
    if user_start > user_end {
        return Ok(format!(
            "❌ Start position {} is greater than end position {}",
            user_start, user_end
        ));
    }

    // Calculate the range we'll actually read
    let user_range_len = user_end - user_start;
    
    // Determine if we need to apply token-aware limiting
    let (effective_end, was_truncated) = match read_limit {
        Some(max_bytes) if user_range_len > max_bytes => {
            (user_start + max_bytes, true)
        }
        _ => (user_end, false),
    };

    // Handle start exceeding file length
    if user_start >= total_file_len {
        // Read last 100 bytes instead
        let fallback_start = total_file_len.saturating_sub(100);
        let content = read_file_range(path, fallback_start, total_file_len)?;
        let line_count = content.lines().count();
        return Ok(format!(
            "{}\n🔍 {} lines read (start {} exceeded length {}, showing last {} chars)",
            content, line_count, user_start, total_file_len, total_file_len - fallback_start
        ));
    }

    // Use optimized seek-based reading
    let content = read_file_range(path, user_start, effective_end)?;
    let line_count = content.lines().count();

    // Format output based on whether truncation occurred
    if was_truncated {
        let context_pct = (ctx.context_used_tokens as f32 / ctx.context_total_tokens as f32 * 100.0) as u32;
        Ok(format!(
            "{}\n🔍 {} lines read (truncated, chars {}-{} of {}, context {}%)",
            content, line_count, user_start, effective_end, total_file_len, context_pct
        ))
    } else if end_was_clamped {
        Ok(format!(
            "{}\n🔍 {} lines read (chars {}-{}, end clamped from {} to file length {})",
            content, line_count, user_start, effective_end, end_char.unwrap(), total_file_len
        ))
    } else if start_char.is_some() || end_char.is_some() {
        Ok(format!(
            "{}\n🔍 {} lines read (chars {}-{})",
            content, line_count, user_start, effective_end
        ))
    } else {
        Ok(format!("{}\n🔍 {} lines read", content, line_count))
    }
}

/// Read a specific byte range from a file using seek (O(1) seek + O(n) read where n = range size).
/// Handles UTF-8 boundary issues by extending the read slightly and trimming invalid chars.
fn read_file_range(path: &Path, start: usize, end: usize) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    
    // For UTF-8 safety, we may need to adjust boundaries.
    // UTF-8 characters are 1-4 bytes, so we read up to 3 extra bytes at start
    // to find a valid character boundary.
    
    // Calculate how far back we might need to look for a char boundary
    let safe_start = start.saturating_sub(3);
    let extra_at_start = start - safe_start;
    
    // Read a few extra bytes at the end to complete any partial char
    let extra_at_end = 3;
    
    // Seek to safe start position
    reader.seek(SeekFrom::Start(safe_start as u64))?;
    
    // Read the extended range
    let bytes_to_read = (end - safe_start) + extra_at_end;
    let mut buffer = vec![0u8; bytes_to_read];
    let bytes_read = reader.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    
    // Convert to string - this should work since we read the whole file originally as UTF-8
    // But we need to find valid boundaries within our extended read
    let full_str = match std::str::from_utf8(&buffer) {
        Ok(s) => s.to_string(),
        Err(_) => {
            // If the whole buffer isn't valid UTF-8, try to find valid boundaries
            // This can happen with binary files or corrupted data
            return Ok(String::from_utf8_lossy(&buffer).into_owned());
        }
    };
    
    // Now we need to trim to the actual requested range
    // We read from safe_start, but user wants from start
    // So we need to skip `extra_at_start` bytes worth of characters
    
    if extra_at_start == 0 && bytes_read <= (end - start) + extra_at_end {
        // Simple case: we started at the right place
        // Just trim any extra at the end
        let target_len = end - start;
        if full_str.len() <= target_len {
            return Ok(full_str);
        }
        // Find char boundary at target_len
        let end_idx = full_str
            .char_indices()
            .take_while(|(i, _)| *i < target_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(full_str.len());
        return Ok(full_str[..end_idx].to_string());
    }
    
    // Complex case: we read extra at the start, need to skip those bytes
    // Find the character that starts at or after `extra_at_start` bytes
    let start_idx = full_str
        .char_indices()
        .find(|(i, _)| *i >= extra_at_start)
        .map(|(i, _)| i)
        .unwrap_or(0);
    
    // Calculate target end based on original request
    let target_byte_len = end - start;
    let end_idx = full_str
        .char_indices()
        .take_while(|(i, _)| *i < start_idx + target_byte_len)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(full_str.len());
    
    Ok(full_str[start_idx..end_idx.min(full_str.len())].to_string())
}

/// Execute the `read_image` tool.
pub async fn execute_read_image<W: UiWriter>(
    tool_call: &ToolCall,
    ctx: &mut ToolContext<'_, W>,
) -> Result<String> {
    debug!("Processing read_image tool call");

    // Get paths from file_paths array
    let mut paths: Vec<String> = Vec::new();

    if let Some(file_paths) = tool_call.args.get("file_paths") {
        if let Some(arr) = file_paths.as_array() {
            for p in arr {
                if let Some(s) = p.as_str() {
                    paths.push(s.to_string());
                }
            }
        }
    }

    if paths.is_empty() {
        return Ok("❌ Missing or empty file_paths argument".to_string());
    }

    let mut results: Vec<String> = Vec::new();
    let mut success_count = 0;

    // Print └─ and newline before images to break out of tool output box
    println!("└─\n");

    for path_str in &paths {
        // Expand tilde (~) to home directory
        let expanded_path = shellexpand::tilde(path_str);
        // Try to resolve with Unicode space fallback (macOS uses U+202F in screenshot names)
        let resolved_path = resolve_path_with_unicode_fallback(expanded_path.as_ref());
        let path = std::path::Path::new(resolved_path.as_ref());

        // Check file exists
        if !path.exists() {
            results.push(format!("❌ Image file not found: {}", path_str));
            continue;
        }

        // Read the file first, then detect format from magic bytes
        match std::fs::read(path) {
            Ok(bytes) => {
                // Detect media type from magic bytes (file signature)
                let media_type = match g3_providers::ImageContent::media_type_from_bytes(&bytes) {
                    Some(mt) => mt,
                    None => {
                        // Fall back to extension-based detection
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        match g3_providers::ImageContent::media_type_from_extension(ext) {
                            Some(mt) => mt,
                            None => {
                                results.push(format!(
                                    "❌ {}: Unsupported or unrecognized image format",
                                    path_str
                                ));
                                continue;
                            }
                        }
                    }
                };

                let file_size = bytes.len();

                // Try to get image dimensions
                let dimensions = get_image_dimensions(&bytes, media_type);

                // Build info string
                let dim_str = dimensions
                    .map(|(w, h)| format!("{}x{}", w, h))
                    .unwrap_or_else(|| "unknown".to_string());

                let size_str = if file_size >= 1024 * 1024 {
                    format!("{:.1} MB", file_size as f64 / (1024.0 * 1024.0))
                } else if file_size >= 1024 {
                    format!("{:.1} KB", file_size as f64 / 1024.0)
                } else {
                    format!("{} bytes", file_size)
                };

                // Output imgcat inline image to terminal (height constrained)
                print_imgcat(&bytes, path_str, &dim_str, media_type, &size_str, 5);

                // Store the image to be attached to the next user message
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let image = g3_providers::ImageContent::new(media_type, encoded);
                ctx.pending_images.push(image);

                success_count += 1;
            }
            Err(e) => {
                results.push(format!("❌ Failed to read '{}': {}", path_str, e));
            }
        }
    }

    // Print ┌─ to resume tool output box
    print!("┌─\n");

    let summary = if success_count == paths.len() {
        format!("{} image(s) read.", success_count)
    } else {
        format!("{}/{} image(s) read.", success_count, paths.len())
    };

    // Only include error results if there are any
    if results.is_empty() {
        Ok(summary)
    } else {
        Ok(format!("{}\n{}", results.join("\n"), summary))
    }
}

/// Execute the `write_file` tool.
pub async fn execute_write_file<W: UiWriter>(
    tool_call: &ToolCall,
    _ctx: &ToolContext<'_, W>,
) -> Result<String> {
    debug!("Processing write_file tool call");
    debug!("Raw tool_call.args: {:?}", tool_call.args);

    // Try multiple argument formats that different providers might use
    let (path_str, content_str) = extract_path_and_content(&tool_call.args);

    debug!(
        "Final extracted values: path_str={:?}, content_str_len={:?}",
        path_str,
        content_str.map(|c| c.len())
    );

    if let (Some(path), Some(content)) = (path_str, content_str) {
        // Expand tilde (~) to home directory
        let expanded_path = shellexpand::tilde(path);
        let path = expanded_path.as_ref();

        debug!("Writing to file: {}", path);

        // Create parent directories if they don't exist
        if let Some(parent) = std::path::Path::new(path).parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Ok(format!(
                    "❌ Failed to create parent directories for '{}': {}",
                    path, e
                ));
            }
        }

        match std::fs::write(path, content) {
            Ok(()) => {
                let line_count = content.lines().count();
                let char_count = content.len();
                let char_display = if char_count >= 1000 {
                    format!("{:.1}k", char_count as f64 / 1000.0)
                } else {
                    format!("{}", char_count)
                };
                Ok(format!(
                    "wrote {} lines | {} chars",
                    line_count, char_display
                ))
            }
            Err(e) => Ok(format!("❌ Failed to write to file '{}': {}", path, e)),
        }
    } else {
        // Provide more detailed error information
        let available_keys = if let Some(obj) = tool_call.args.as_object() {
            obj.keys().collect::<Vec<_>>()
        } else {
            vec![]
        };

        Ok(format!(
            "❌ Missing file_path or content argument. Available keys: {:?}. Expected formats: {{\"file_path\": \"...\", \"content\": \"...\"}}, {{\"path\": \"...\", \"content\": \"...\"}}, {{\"filename\": \"...\", \"text\": \"...\"}}, or {{\"file\": \"...\", \"data\": \"...\"}}",
            available_keys
        ))
    }
}

/// Execute the `str_replace` tool.
pub async fn execute_str_replace<W: UiWriter>(
    tool_call: &ToolCall,
    _ctx: &ToolContext<'_, W>,
) -> Result<String> {
    debug!("Processing str_replace tool call");

    let args_obj = match tool_call.args.as_object() {
        Some(obj) => obj,
        None => return Ok("❌ Invalid arguments: expected object".to_string()),
    };

    let file_path = match args_obj.get("file_path").and_then(|v| v.as_str()) {
        Some(path) => {
            let expanded_path = shellexpand::tilde(path);
            expanded_path.into_owned()
        }
        None => return Ok("❌ Missing or invalid file_path argument".to_string()),
    };

    let diff = match args_obj.get("diff").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return Ok("❌ Missing or invalid diff argument".to_string()),
    };

    // Optional start and end character positions (0-indexed, end is EXCLUSIVE)
    let start_char = args_obj
        .get("start")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let end_char = args_obj
        .get("end")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);

    debug!(
        "str_replace: path={}, start={:?}, end={:?}",
        file_path, start_char, end_char
    );

    // Read the existing file
    let file_content = match std::fs::read_to_string(&file_path) {
        Ok(content) => content,
        Err(e) => return Ok(format!("❌ Failed to read file '{}': {}", file_path, e)),
    };

    // Apply unified diff to content
    let result = match apply_unified_diff_to_string(&file_content, diff, start_char, end_char) {
        Ok(r) => r,
        Err(e) => return Ok(format!("❌ {}", e)),
    };

    // Count insertions and deletions from the diff
    let mut insertions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            insertions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }

    // Write the result back to the file
    match std::fs::write(&file_path, &result) {
        Ok(()) => Ok(format!("✅ \x1b[32m+{} insertions\x1b[0m | \x1b[31m-{} deletions\x1b[0m", insertions, deletions)),
        Err(e) => Ok(format!("❌ Failed to write to file '{}': {}", file_path, e)),
    }
}

// Helper functions

/// Known argument key pairs for path and content.
const PATH_CONTENT_KEYS: &[(&str, &str)] = &[
    ("file_path", "content"),  // Standard format
    ("path", "content"),       // Anthropic-style
    ("filename", "text"),      // Alternative naming
    ("file", "data"),          // Alternative naming
];

/// Extract path and content from various argument formats.
fn extract_path_and_content(args: &serde_json::Value) -> (Option<&str>, Option<&str>) {
    match args {
        serde_json::Value::Object(obj) => {
            for &(path_key, content_key) in PATH_CONTENT_KEYS {
                if let (Some(p), Some(c)) = (obj.get(path_key), obj.get(content_key)) {
                    if let (Some(path), Some(content)) = (p.as_str(), c.as_str()) {
                        return (Some(path), Some(content));
                    }
                }
            }
            (None, None)
        }
        serde_json::Value::Array(arr) if arr.len() >= 2 => {
            match (arr[0].as_str(), arr[1].as_str()) {
                (Some(path), Some(content)) => (Some(path), Some(content)),
                _ => (None, None),
            }
        }
        _ => (None, None),
    }
}

/// Get image dimensions from raw bytes.
pub fn get_image_dimensions(bytes: &[u8], media_type: &str) -> Option<(u32, u32)> {
    match media_type {
        "image/png" => {
            // PNG: width at bytes 16-19, height at bytes 20-23 (big-endian)
            if bytes.len() >= 24 {
                let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
                let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
                Some((width, height))
            } else {
                None
            }
        }
        "image/jpeg" => {
            // JPEG: Need to find SOF0/SOF2 marker (FF C0 or FF C2)
            let mut i = 2; // Skip FF D8
            while i + 8 < bytes.len() {
                if bytes[i] == 0xFF {
                    let marker = bytes[i + 1];
                    // SOF0, SOF1, SOF2 markers contain dimensions
                    if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
                        let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                        let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                        return Some((width, height));
                    }
                    // Skip to next marker
                    if marker == 0xD8
                        || marker == 0xD9
                        || marker == 0x01
                        || (0xD0..=0xD7).contains(&marker)
                    {
                        i += 2;
                    } else {
                        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                        i += 2 + len;
                    }
                } else {
                    i += 1;
                }
            }
            None
        }
        "image/gif" => {
            // GIF: width at bytes 6-7, height at bytes 8-9 (little-endian)
            if bytes.len() >= 10 {
                let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
                let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
                Some((width, height))
            } else {
                None
            }
        }
        "image/webp" => {
            // WebP VP8: dimensions at specific offsets (simplified)
            if bytes.len() >= 30 && &bytes[12..16] == b"VP8 " {
                let width = (u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF) as u32;
                let height = (u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF) as u32;
                Some((width, height))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Print image using iTerm2 imgcat protocol with info line.
pub fn print_imgcat(
    bytes: &[u8],
    name: &str,
    dimensions: &str,
    media_type: &str,
    size: &str,
    max_height: u32,
) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    // Extract just the filename from the path
    let filename = std::path::Path::new(name)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(name);
    // iTerm2 inline image protocol (single space prefix)
    print!(
        " \x1b]1337;File=inline=1;height={};name={}:{}\x07\n",
        max_height, name, encoded
    );
    // Print dimmed info line with filename only (no │ prefix)
    println!(
        " \x1b[2m{} | {} | {} | {}\x1b[0m",
        filename, dimensions, media_type, size
    );
    // Blank line before next image (no │ prefix)
    println!();
}
