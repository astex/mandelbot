use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:",
            // Scheme-prefixed URLs: http(s)://, file:///
            r"(?:https?://|file:///)\S+",
        r"|",
            // www-prefixed
            r"www\.\S+",
        r"|",
            // GitHub shorthand: org/repo#123
            r"[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+#[0-9]+",
        r"|",
            // Bare domains with common TLDs
            r"[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?",
            r"(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?)*",
            r"\.(?:com|org|net|io|dev|co|edu|gov|mil|info|biz|me|app|xyz|tech|cloud|ai|rs|py|js|ts|sh|cc)",
            r"(?:/\S*)?",
        r")",
    ))
    .unwrap()
});

pub struct UrlMatch {
    /// The URL to open (with scheme prepended if needed).
    pub url: String,
    /// Character offset of match start in the input text.
    pub start: usize,
    /// Character offset of match end (exclusive) in the input text.
    pub end: usize,
}

/// Find the URL (if any) that contains the given character offset in `text`.
pub fn find_url_at(text: &str, char_offset: usize) -> Option<UrlMatch> {
    for m in URL_RE.find_iter(text) {
        let mut byte_end = m.end();

        // Strip trailing punctuation that's likely not part of the URL.
        while byte_end > m.start() {
            let last = text.as_bytes()[byte_end - 1];
            if matches!(last, b'.' | b',' | b')' | b']' | b'\'' | b'"' | b';' | b':') {
                byte_end -= 1;
            } else {
                break;
            }
        }

        let start_char = text[..m.start()].chars().count();
        let end_char = start_char + text[m.start()..byte_end].chars().count();

        if start_char <= char_offset && char_offset < end_char {
            let matched = &text[m.start()..byte_end];
            let url = if matched.starts_with("http://")
                || matched.starts_with("https://")
                || matched.starts_with("file:///")
            {
                matched.to_string()
            } else if let Some((repo, number)) = matched.split_once('#') {
                let org = repo.split('/').next().unwrap_or("");
                if repo.contains('/') && !org.contains('.') && number.chars().all(|c| c.is_ascii_digit()) {
                    format!("https://github.com/{repo}/pull/{number}")
                } else {
                    format!("https://{matched}")
                }
            } else {
                format!("https://{matched}")
            };
            return Some(UrlMatch { url, start: start_char, end: end_char });
        }
    }
    None
}

/// Parse an `owner/repo` slug from a GitHub remote URL in either
/// SSH (`git@github.com:owner/repo.git`) or HTTPS
/// (`https://github.com/owner/repo(.git)`) form.
pub fn parse_github_slug(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let tail = if let Some(rest) =
        remote.strip_prefix("git@github.com:")
    {
        rest
    } else if let Some(rest) =
        remote.strip_prefix("https://github.com/")
    {
        rest
    } else if let Some(rest) =
        remote.strip_prefix("ssh://git@github.com/")
    {
        rest
    } else {
        return None;
    };
    let tail = tail.strip_suffix(".git").unwrap_or(tail);
    let (owner, repo) = tail.split_once('/')?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// Look up the `owner/repo` slug for the GitHub `origin` remote of
/// the git repository containing `dir`.  Returns `None` if `dir` is
/// not in a git repo, has no `origin`, or the remote isn't GitHub.
pub fn github_slug_for_dir(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8(output.stdout).ok()?;
    parse_github_slug(&remote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_url() {
        let text = "visit https://example.com/path today";
        let m = find_url_at(text, 6).unwrap();
        assert_eq!(m.url, "https://example.com/path");
        assert_eq!(m.start, 6);
        assert_eq!(m.end, 30);
    }

    #[test]
    fn www_url() {
        let text = "go to www.therapykit.com now";
        let m = find_url_at(text, 6).unwrap();
        assert_eq!(m.url, "https://www.therapykit.com");
    }

    #[test]
    fn bare_domain() {
        let text = "check therapykit.com for info";
        let m = find_url_at(text, 6).unwrap();
        assert_eq!(m.url, "https://therapykit.com");
    }

    #[test]
    fn file_url() {
        let text = "open file:///home/user/doc.html please";
        let m = find_url_at(text, 5).unwrap();
        assert_eq!(m.url, "file:///home/user/doc.html");
    }

    #[test]
    fn trailing_punctuation_stripped() {
        let text = "see https://example.com.";
        let m = find_url_at(text, 4).unwrap();
        assert_eq!(m.url, "https://example.com");
    }

    #[test]
    fn no_match_outside() {
        let text = "visit https://example.com today";
        assert!(find_url_at(text, 0).is_none());
        assert!(find_url_at(text, 30).is_none());
    }

    #[test]
    fn bare_domain_with_path() {
        let text = "go to therapykit.com/pricing now";
        let m = find_url_at(text, 6).unwrap();
        assert_eq!(m.url, "https://therapykit.com/pricing");
    }

    #[test]
    fn github_pr_shorthand() {
        let text = "see anthropics/claude-code#100 for details";
        let m = find_url_at(text, 4).unwrap();
        assert_eq!(m.url, "https://github.com/anthropics/claude-code/pull/100");
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 30);
    }

    #[test]
    fn github_pr_shorthand_at_end() {
        let text = "fixed in astex/mandelbot#73.";
        let m = find_url_at(text, 9).unwrap();
        assert_eq!(m.url, "https://github.com/astex/mandelbot/pull/73");
    }

    #[test]
    fn github_pr_shorthand_dotted_repo() {
        let text = "see acme/example.com#42 for details";
        let m = find_url_at(text, 4).unwrap();
        assert_eq!(m.url, "https://github.com/acme/example.com/pull/42");
    }

    #[test]
    fn parse_slug_ssh() {
        assert_eq!(
            parse_github_slug("git@github.com:astex/mandelbot.git"),
            Some("astex/mandelbot".into()),
        );
    }

    #[test]
    fn parse_slug_https() {
        assert_eq!(
            parse_github_slug("https://github.com/astex/mandelbot.git"),
            Some("astex/mandelbot".into()),
        );
    }

    #[test]
    fn parse_slug_https_no_git_suffix() {
        assert_eq!(
            parse_github_slug("https://github.com/astex/mandelbot"),
            Some("astex/mandelbot".into()),
        );
    }

    #[test]
    fn parse_slug_rejects_non_github() {
        assert_eq!(
            parse_github_slug("git@gitlab.com:astex/mandelbot.git"),
            None,
        );
    }
}
