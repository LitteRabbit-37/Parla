// Strip des balises de raisonnement que certains modeles emettent malgre
// les instructions ("output only cleaned text").
//
// Reference VoiceInk : VoiceInk/Services/AIEnhancement/AIEnhancementService.swift
// L~330-360 (AIEnhancementOutputFilter.filter).
// Regex utilisees cote VoiceInk :
//   (?s)<thinking>.*?</thinking>
//   (?s)<think>.*?</think>
//   (?s)<reasoning>.*?</reasoning>
// Puis trim.

use std::sync::LazyLock;

use regex::Regex;

static RE_THINKING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<thinking>.*?</thinking>").unwrap());
static RE_THINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<think>.*?</think>").unwrap());
static RE_REASONING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<reasoning>.*?</reasoning>").unwrap());

pub fn filter(input: &str) -> String {
    let s = RE_THINKING.replace_all(input, "");
    let s = RE_THINK.replace_all(&s, "");
    let s = RE_REASONING.replace_all(&s, "");
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_thinking_block() {
        let s = filter("<thinking>hmm</thinking>Hello world");
        assert_eq!(s, "Hello world");
    }

    #[test]
    fn removes_multiline_think() {
        let s = filter("<think>\nline1\nline2\n</think>\nResult");
        assert_eq!(s, "Result");
    }

    #[test]
    fn removes_reasoning_and_trims() {
        let s = filter("  <reasoning>x</reasoning> Hi ");
        assert_eq!(s, "Hi");
    }

    #[test]
    fn leaves_unrelated_tags_alone() {
        let s = filter("<code>do</code>");
        assert_eq!(s, "<code>do</code>");
    }
}
