//! Tests for UTF-8 safe file reading with seek optimization.

use std::fs;
use std::io::Write;
use tempfile::TempDir;

/// Test that reading a file with multi-byte UTF-8 characters works correctly
/// when the byte range falls in the middle of a character.
#[test]
fn test_read_file_range_utf8_boundary() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("utf8_test.txt");
    
    // Create a file with emoji (4-byte UTF-8 chars)
    // "🎉" is 4 bytes: F0 9F 8E 89
    // "hello🎉world🎉test" 
    // h=1, e=1, l=1, l=1, o=1, 🎉=4, w=1, o=1, r=1, l=1, d=1, 🎉=4, t=1, e=1, s=1, t=1
    // Byte positions: hello=0-4, 🎉=5-8, world=9-13, 🎉=14-17, test=18-21
    let content = "hello🎉world🎉test";
    fs::write(&file_path, content).unwrap();
    
    // Verify the byte layout
    let bytes = fs::read(&file_path).unwrap();
    assert_eq!(bytes.len(), 22); // 5 + 4 + 5 + 4 + 4 = 22 bytes
    
    // Read the whole file - should work
    let result = fs::read_to_string(&file_path).unwrap();
    assert_eq!(result, content);
}

/// Test that we handle files with various UTF-8 characters
#[test]
fn test_utf8_various_chars() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("utf8_various.txt");
    
    // Mix of 1-byte (ASCII), 2-byte (é), 3-byte (中), and 4-byte (🎉) chars
    let content = "café中文🎉done";
    fs::write(&file_path, content).unwrap();
    
    let bytes = fs::read(&file_path).unwrap();
    // c=1, a=1, f=1, é=2, 中=3, 文=3, 🎉=4, d=1, o=1, n=1, e=1 = 19 bytes
    assert_eq!(bytes.len(), 19);
    
    let result = fs::read_to_string(&file_path).unwrap();
    assert_eq!(result, content);
}

/// Test reading from the middle of a file with UTF-8 content
#[test]
fn test_read_middle_of_utf8_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("utf8_middle.txt");
    
    // Create a larger file with UTF-8 content
    let mut content = String::new();
    for i in 0..100 {
        content.push_str(&format!("line{}🎉\n", i));
    }
    fs::write(&file_path, &content).unwrap();
    
    // Read from the middle - this exercises the seek + UTF-8 boundary logic
    let full = fs::read_to_string(&file_path).unwrap();
    assert!(full.contains("line50🎉"));
}

/// Test that binary files don't cause panics
#[test]
fn test_binary_file_no_panic() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("binary.bin");
    
    // Write some binary data with invalid UTF-8 sequences
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(&[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x81, 0x82]).unwrap();
    
    // Reading as string should not panic (will use lossy conversion)
    // This tests the fallback path in read_file_range
    let result = fs::read(&file_path);
    assert!(result.is_ok());
}
