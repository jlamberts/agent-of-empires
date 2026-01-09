//! tmux session management

use anyhow::{bail, Result};
use std::process::Command;

use super::{session_exists_from_cache, SESSION_PREFIX};
use crate::session::Status;

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const CLAUDE_WHIMSICAL_WORDS: &[&str] = &[
    "accomplishing", "actioning", "actualizing", "baking", "booping",
    "brewing", "calculating", "cerebrating", "channelling", "churning",
    "clauding", "coalescing", "cogitating", "combobulating", "computing",
    "concocting", "conjuring", "considering", "contemplating", "cooking",
    "crafting", "creating", "crunching", "deciphering", "deliberating",
    "determining", "discombobulating", "divining", "doing", "effecting",
    "elucidating", "enchanting", "envisioning", "finagling", "flibbertigibbeting",
    "forging", "forming", "frolicking", "generating", "germinating",
    "hatching", "herding", "honking", "hustling", "ideating",
    "imagining", "incubating", "inferring", "jiving", "manifesting",
    "marinating", "meandering", "moseying", "mulling", "mustering",
    "musing", "noodling", "percolating", "perusing", "philosophising",
    "pondering", "pontificating", "processing", "puttering", "puzzling",
    "reticulating", "ruminating", "scheming", "schlepping", "shimmying",
    "shucking", "simmering", "smooshing", "spelunking", "spinning",
    "stewing", "sussing", "synthesizing", "thinking", "tinkering",
    "transmuting", "unfurling", "unravelling", "vibing", "wandering",
    "whirring", "wibbling", "wizarding", "working", "wrangling",
];

fn strip_ansi(content: &str) -> String {
    let mut result = content.to_string();

    // Remove CSI sequences: ESC [ ... letter
    while let Some(start) = result.find("\x1b[") {
        let rest = &result[start + 2..];
        let end_offset = rest
            .find(|c: char| c.is_ascii_alphabetic())
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        result = format!("{}{}", &result[..start], &result[start + 2 + end_offset..]);
    }

    // Remove OSC sequences: ESC ] ... BEL
    while let Some(start) = result.find("\x1b]") {
        if let Some(end) = result[start..].find('\x07') {
            result = format!("{}{}", &result[..start], &result[start + end + 1..]);
        } else {
            break;
        }
    }

    result
}

pub struct Session {
    name: String,
}

impl Session {
    pub fn new(id: &str, title: &str) -> Result<Self> {
        Ok(Self {
            name: Self::generate_name(id, title),
        })
    }

    pub fn generate_name(id: &str, title: &str) -> String {
        let safe_title = sanitize_session_name(title);
        let short_id = if id.len() > 8 { &id[..8] } else { id };
        format!("{}{}_{}", SESSION_PREFIX, safe_title, short_id)
    }

    pub fn exists(&self) -> bool {
        // Try cache first
        if let Some(exists) = session_exists_from_cache(&self.name) {
            return exists;
        }

        // Fallback to direct check
        Command::new("tmux")
            .args(["has-session", "-t", &self.name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn create(&self, working_dir: &str, command: Option<&str>) -> Result<()> {
        if self.exists() {
            return Ok(());
        }

        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            self.name.clone(),
            "-c".to_string(),
            working_dir.to_string(),
        ];

        if let Some(cmd) = command {
            args.push(cmd.to_string());
        }

        let output = Command::new("tmux")
            .args(&args)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create tmux session: {}", stderr);
        }

        // Register in cache
        super::refresh_session_cache();

        Ok(())
    }

    pub fn kill(&self) -> Result<()> {
        if !self.exists() {
            return Ok(());
        }

        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.name])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to kill tmux session: {}", stderr);
        }

        Ok(())
    }

    pub fn attach(&self) -> Result<()> {
        if !self.exists() {
            bail!("Session does not exist: {}", self.name);
        }

        // Check if we're already in tmux
        if std::env::var("TMUX").is_ok() {
            // Switch to session
            let status = Command::new("tmux")
                .args(["switch-client", "-t", &self.name])
                .status()?;

            if !status.success() {
                bail!("Failed to switch to tmux session");
            }
        } else {
            // Attach to session
            let status = Command::new("tmux")
                .args(["attach-session", "-t", &self.name])
                .status()?;

            if !status.success() {
                bail!("Failed to attach to tmux session");
            }
        }

        Ok(())
    }

    pub fn capture_pane(&self, lines: usize) -> Result<String> {
        if !self.exists() {
            return Ok(String::new());
        }

        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &self.name,
                "-p",
                "-S",
                &format!("-{}", lines),
            ])
            .output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Ok(String::new())
        }
    }

    pub fn detect_status(&self, tool: &str) -> Result<Status> {
        let content = self.capture_pane(50)?;
        Ok(detect_status_from_content(&content, tool))
    }

}

fn sanitize_session_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(20)
        .collect()
}

fn detect_status_from_content(content: &str, tool: &str) -> Status {
    let lines: Vec<&str> = content.lines().collect();
    let last_lines = if lines.len() > 10 {
        &lines[lines.len() - 10..]
    } else {
        &lines
    };
    let last_content = last_lines.join("\n");
    let last_content_lower = last_content.to_lowercase();

    match tool {
        "claude" => detect_claude_status(&last_content),
        "gemini" => detect_gemini_status(&last_content_lower),
        "opencode" => detect_opencode_status(&last_content_lower),
        "codex" => detect_generic_status(&last_content_lower),
        _ => detect_shell_status(&last_content_lower),
    }
}

fn detect_claude_status(content: &str) -> Status {
    let lines: Vec<&str> = content.lines().collect();
    let last_15: Vec<&str> = lines.iter().rev().take(15).filter(|l| !l.trim().is_empty()).copied().collect();
    let recent_content = last_15.iter().rev().cloned().collect::<Vec<_>>().join("\n");
    let recent_lower = recent_content.to_lowercase();

    // BUSY indicators - check FIRST (if busy, definitely not waiting)
    let busy_indicators = [
        "esc to interrupt",
        "(esc to interrupt)",
        "· esc to interrupt",
    ];
    for indicator in &busy_indicators {
        if recent_lower.contains(indicator) {
            return Status::Running;
        }
    }

    // Check for spinner characters in last 5 lines
    let last_5: Vec<&str> = lines.iter().rev().take(5).copied().collect();
    for line in &last_5 {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    // Check for whimsical words + "tokens" pattern (e.g., "thinking... (25s · 340 tokens)")
    if recent_lower.contains("tokens") {
        for word in CLAUDE_WHIMSICAL_WORDS {
            if recent_lower.contains(word) {
                return Status::Running;
            }
        }
    }

    // Check for "thinking" or "connecting" with tokens (common Claude status)
    if (recent_lower.contains("thinking") || recent_lower.contains("connecting"))
        && recent_lower.contains("tokens") {
        return Status::Running;
    }

    // WAITING indicators - Permission prompts
    let permission_prompts = [
        "No, and tell Claude what to do differently",
        "Yes, allow once",
        "Yes, allow always",
        "Allow once",
        "Allow always",
        "│ Do you want",
        "│ Would you like",
        "│ Allow",
        "❯ Yes",
        "❯ No",
        "❯ Allow",
        "Do you trust the files in this folder?",
        "Allow this MCP server",
        "Run this command?",
        "Execute this?",
    ];
    for prompt in &permission_prompts {
        if content.contains(prompt) {
            return Status::Waiting;
        }
    }

    // WAITING - Check if last non-empty line is ">" input prompt
    if let Some(last_line) = lines.iter().rev().find(|l| !l.trim().is_empty()) {
        let clean_line = strip_ansi(last_line).trim().to_string();
        if clean_line == ">" || clean_line == "> " {
            return Status::Waiting;
        }
        // Check for prompt with partial user input (user started typing)
        if clean_line.starts_with("> ") && !clean_line.contains("esc") && clean_line.len() < 100 {
            return Status::Waiting;
        }
    }

    // WAITING - Question prompts
    let question_prompts = [
        "Continue?",
        "Proceed?",
        "(Y/n)",
        "(y/N)",
        "[Y/n]",
        "[y/N]",
        "(yes/no)",
        "[yes/no]",
        "Approve this plan?",
        "Execute plan?",
    ];
    for prompt in &question_prompts {
        if recent_content.contains(prompt) {
            return Status::Waiting;
        }
    }

    // WAITING - Completion indicators + ">" prompt nearby
    let completion_indicators = [
        "task completed",
        "done!",
        "finished",
        "what would you like",
        "what else",
        "anything else",
        "let me know if",
    ];
    let has_completion = completion_indicators.iter().any(|ind| recent_lower.contains(ind));
    if has_completion {
        for line in &last_5 {
            let clean = strip_ansi(line).trim().to_string();
            if clean == ">" || clean == "> " {
                return Status::Waiting;
            }
        }
    }

    Status::Idle
}

fn detect_gemini_status(content: &str) -> Status {
    let waiting_patterns = [
        "gemini>",
        "> ",
        "enter your",
        "type your",
    ];

    let running_patterns = [
        "generating",
        "thinking",
        "processing",
    ];

    for pattern in &running_patterns {
        if content.contains(pattern) {
            return Status::Running;
        }
    }

    for pattern in &waiting_patterns {
        if content.contains(pattern) {
            return Status::Waiting;
        }
    }

    Status::Idle
}

fn detect_opencode_status(content: &str) -> Status {
    let lines: Vec<&str> = content.lines().collect();
    let last_15: Vec<&str> = lines.iter().rev().take(15).filter(|l| !l.trim().is_empty()).copied().collect();
    let recent_content = last_15.iter().rev().cloned().collect::<Vec<_>>().join("\n");
    let recent_lower = recent_content.to_lowercase();

    // RUNNING indicators - check FIRST
    let busy_indicators = [
        "ctrl+c",
        "ctrl-c",
        "escape to cancel",
        "esc to cancel",
        "press esc",
        "to interrupt",
        "to cancel",
    ];
    for indicator in &busy_indicators {
        if recent_lower.contains(indicator) {
            return Status::Running;
        }
    }

    // Check for spinner characters in last 5 lines
    let last_5: Vec<&str> = lines.iter().rev().take(5).copied().collect();
    for line in &last_5 {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    // Running status words
    let running_patterns = [
        "generating",
        "thinking",
        "working",
        "processing",
        "loading",
        "executing",
        "running",
        "streaming",
        "writing",
        "reading",
        "analyzing",
        "compiling",
    ];
    for pattern in &running_patterns {
        if recent_lower.contains(pattern) && !recent_lower.contains("finished") {
            return Status::Running;
        }
    }

    // Check for progress indicators (ellipsis animation)
    if recent_lower.contains("...") && !recent_lower.contains("done") && !recent_lower.contains("complete") {
        let has_status_word = ["wait", "load", "process", "think", "generat", "work"]
            .iter()
            .any(|w| recent_lower.contains(w));
        if has_status_word {
            return Status::Running;
        }
    }

    // WAITING indicators - Permission/confirmation prompts
    let permission_prompts = [
        "allow",
        "approve",
        "confirm",
        "accept",
        "deny",
        "reject",
        "yes/no",
        "y/n",
        "[y/n]",
        "(y/n)",
        "[yes/no]",
        "(yes/no)",
        "continue?",
        "proceed?",
        "execute?",
        "run this",
        "apply changes",
        "do you want",
        "would you like",
    ];
    for prompt in &permission_prompts {
        if recent_lower.contains(prompt) {
            return Status::Waiting;
        }
    }

    // WAITING - Check if last non-empty line is input prompt
    if let Some(last_line) = lines.iter().rev().find(|l| !l.trim().is_empty()) {
        let clean_line = strip_ansi(last_line).trim().to_string();
        let clean_lower = clean_line.to_lowercase();

        // Common opencode input prompts
        if clean_line == ">" || clean_line == "> " || clean_line == ">>" {
            return Status::Waiting;
        }
        if clean_lower.starts_with("> ") && !clean_lower.contains("esc") && clean_line.len() < 100 {
            return Status::Waiting;
        }
        // Check for prompt patterns like "user:" or "input:"
        if clean_lower.ends_with(":") || clean_lower.ends_with(": ") {
            let prompt_words = ["input", "user", "you", "message", "query", "prompt"];
            if prompt_words.iter().any(|w| clean_lower.contains(w)) {
                return Status::Waiting;
            }
        }
    }

    // WAITING - Completion indicators + input prompt nearby
    let completion_indicators = [
        "complete",
        "done",
        "finished",
        "ready",
        "what would you like",
        "what else",
        "anything else",
        "how can i help",
        "let me know",
    ];
    let has_completion = completion_indicators.iter().any(|ind| recent_lower.contains(ind));
    if has_completion {
        for line in &last_5 {
            let clean = strip_ansi(line).trim().to_string();
            if clean == ">" || clean == "> " || clean == ">>" {
                return Status::Waiting;
            }
        }
    }

    Status::Idle
}

fn detect_generic_status(content: &str) -> Status {
    let running_patterns = [
        "running",
        "processing",
        "loading",
        "thinking",
    ];

    for pattern in &running_patterns {
        if content.contains(pattern) {
            return Status::Running;
        }
    }

    // Check for common prompts
    if content.ends_with("$ ") || content.ends_with("> ") || content.ends_with("# ") {
        return Status::Waiting;
    }

    Status::Idle
}

fn detect_shell_status(content: &str) -> Status {
    // Shell prompts
    if content.ends_with("$ ") || content.ends_with("> ") || content.ends_with("# ") || content.ends_with("% ") {
        return Status::Waiting;
    }

    // Running if we see a spinner or progress indicator
    let running_indicators = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "...", "───"];
    for indicator in &running_indicators {
        if content.contains(indicator) {
            return Status::Running;
        }
    }

    Status::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_session_name() {
        assert_eq!(sanitize_session_name("my-project"), "my-project");
        assert_eq!(sanitize_session_name("my project"), "my_project");
        assert_eq!(sanitize_session_name("a".repeat(30).as_str()).len(), 20);
    }

    #[test]
    fn test_generate_name() {
        let name = Session::generate_name("abc123def456", "My Project");
        assert!(name.starts_with(SESSION_PREFIX));
        assert!(name.contains("My_Project"));
        assert!(name.contains("abc123de"));
    }

    #[test]
    fn test_detect_claude_status_running() {
        // "esc to interrupt" indicates Claude is actively working
        assert_eq!(detect_claude_status("Working on your request (esc to interrupt)"), Status::Running);
        assert_eq!(detect_claude_status("Thinking... · esc to interrupt"), Status::Running);

        // Spinner characters indicate active processing
        assert_eq!(detect_claude_status("Processing ⠋"), Status::Running);
        assert_eq!(detect_claude_status("Loading ⠹"), Status::Running);

        // Whimsical words + tokens pattern
        assert_eq!(detect_claude_status("Flibbertigibbeting... (25s · 340 tokens)"), Status::Running);
        assert_eq!(detect_claude_status("Thinking... (10s · 100 tokens)"), Status::Running);
    }

    #[test]
    fn test_detect_claude_status_waiting() {
        // Permission prompts
        assert_eq!(detect_claude_status("Yes, allow once\nNo, and tell Claude what to do differently"), Status::Waiting);
        assert_eq!(detect_claude_status("Do you trust the files in this folder?"), Status::Waiting);

        // Input prompt
        assert_eq!(detect_claude_status("Task complete.\n>"), Status::Waiting);
        assert_eq!(detect_claude_status("Done!\n> "), Status::Waiting);

        // Question prompts
        assert_eq!(detect_claude_status("Continue? (Y/n)"), Status::Waiting);
    }

    #[test]
    fn test_detect_claude_status_idle() {
        // No indicators = idle
        assert_eq!(detect_claude_status("completed the task"), Status::Idle);
        assert_eq!(detect_claude_status("some random output"), Status::Idle);
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("no codes here"), "no codes here");
        assert_eq!(strip_ansi("\x1b[1;34mbold blue\x1b[0m"), "bold blue");
    }

    #[test]
    fn test_detect_opencode_status_running() {
        // Interrupt hints indicate active processing
        assert_eq!(detect_opencode_status("Processing your request (ctrl+c to cancel)"), Status::Running);
        assert_eq!(detect_opencode_status("Working... press esc to interrupt"), Status::Running);

        // Spinner characters indicate active processing
        assert_eq!(detect_opencode_status("Generating ⠋"), Status::Running);
        assert_eq!(detect_opencode_status("Loading ⠹"), Status::Running);

        // Running status words
        assert_eq!(detect_opencode_status("Generating response..."), Status::Running);
        assert_eq!(detect_opencode_status("Thinking about your question"), Status::Running);
        assert_eq!(detect_opencode_status("Processing request"), Status::Running);
        assert_eq!(detect_opencode_status("Working on it..."), Status::Running);
        assert_eq!(detect_opencode_status("Streaming response"), Status::Running);
    }

    #[test]
    fn test_detect_opencode_status_waiting() {
        // Permission prompts
        assert_eq!(detect_opencode_status("Allow this action? [y/n]"), Status::Waiting);
        assert_eq!(detect_opencode_status("Do you want to continue?"), Status::Waiting);
        assert_eq!(detect_opencode_status("Approve changes (yes/no)"), Status::Waiting);

        // Input prompt
        assert_eq!(detect_opencode_status("Task complete.\n>"), Status::Waiting);
        assert_eq!(detect_opencode_status("Ready for input\n> "), Status::Waiting);

        // Completion + prompt
        assert_eq!(detect_opencode_status("Done! What else can I help with?\n>"), Status::Waiting);
    }

    #[test]
    fn test_detect_opencode_status_idle() {
        // No indicators = idle
        assert_eq!(detect_opencode_status("completed the task"), Status::Idle);
        assert_eq!(detect_opencode_status("some random output"), Status::Idle);
        assert_eq!(detect_opencode_status("file saved successfully"), Status::Idle);
    }
}
