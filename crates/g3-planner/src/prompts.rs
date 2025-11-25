//! Prompts used for discovery phase

/// System prompt for discovery mode - instructs the LLM to analyze codebase and generate exploration commands
pub const DISCOVERY_SYSTEM_PROMPT: &str = r#"You are an expert code analyst. Your task is to analyze a codebase structure and generate shell commands to explore it further.

You will receive:
1. User requirements describing what needs to be implemented
2. A codebase report showing the structure and key elements of the codebase

Your job is to:
1. Understand the requirements and identify what parts of the codebase are relevant
2. Generate shell commands to explore those parts in more detail

IMPORTANT: Do NOT attempt to implement anything. Only generate exploration commands."#;

/// Discovery prompt template - used when we have a codebase report.
/// The codebase report should be appended after this prompt.
pub const DISCOVERY_REQUIREMENTS_PROMPT: &str = r#"**CRITICAL**: DO ABSOLUTELY NOT ATTEMPT TO IMPLEMENT THESE REQUIREMENTS AT THIS POINT. ONLY USE THEM TO
UNDERSTAND WHICH PARTS OF THE CODE YOU MIGHT BE INTERESTED IN, AND WHAT SEARCH/GREP EXPRESSIONS YOU MIGHT WANT TO USE
TO GET A BETTER UNDERSTANDING OF THE CODEBASE.

Your task is to analyze the codebase structure provided below and generate shell commands to explore it further.

Your output MUST include:
1. A section with heading {{SUMMARY BASED ON INITIAL INFO}} containing a brief summary of what you understand about the codebase structure (max 10000 tokens).
2. A section with heading {{CODE EXPLORATION COMMANDS}} containing shell commands to explore the codebase further.
   - Use tools like `ls`, `rg` (ripgrep), `grep`, `sed`, `cat`, `head`, `tail` etc.
   - Focus on commands that will help understand the code structure without dumping entire files.
   - Mark the beginning and end of the commands with "```".

DO NOT ADD ANY COMMENTS OR OTHER EXPLANATION IN THE COMMANDS SECTION, JUST INCLUDE THE SHELL COMMANDS."#;
