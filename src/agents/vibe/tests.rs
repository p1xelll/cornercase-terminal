use super::*;

fn parse(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

mod arguments {
    use super::*;

    #[test]
    fn one_auto_approval_spelling_normalizes_to_the_agent() {
        assert_eq!(normalized(&parse(&["--yolo"])), ["--agent", "auto-approve"]);
        assert_eq!(normalized(&parse(&["--auto-approve"])), ["--agent", "auto-approve"]);
        assert_eq!(normalized(&parse(&["--agent=auto-approve"])), ["--agent", "auto-approve"]);
        assert_eq!(normalized(&parse(&["--smart-approve"])), ["--agent", "smart-approve"]);
        assert_eq!(normalized(&parse(&["--add-dir", "/work"])), ["--add-dir", "/work"]);
    }

    #[test]
    fn resumes_by_session_id() {
        assert_eq!(
            resume(parse(&["--agent", "ask"]), "8c7d".into()),
            Some(parse(&["--agent", "ask", "--resume", "8c7d"]))
        );
    }

    #[test]
    fn recognizes_the_rust_binary_behind_the_launcher() {
        assert!(runs_as("/home/a/.local/share/uv/tools/mistral-vibe/lib/python3.14/site-packages/vibe/_bin/vibe-rs"));
        assert!(!runs_as("vibe"));
        assert!(!runs_as("vibe-app-server"));
    }

    #[test]
    fn shell_teleport_and_slash_commands_stay_local_but_skills_are_prompts() {
        assert!(local_command("!git status"));
        assert!(local_command("&move this to the cloud"));
        assert!(local_command("/compact"));
        assert!(!local_command(" /skill review"));
        assert!(!local_command("fix the login"));
    }
}
