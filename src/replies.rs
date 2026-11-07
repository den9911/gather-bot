pub const PART_UNITS: usize = 3500;
const MAX_PARTS: usize = 4;

pub fn parts(text: &str) -> Vec<String> {
    let mut characters = text.chars().peekable();
    let mut result = Vec::new();
    for index in 0..MAX_PARTS {
        if characters.peek().is_none() {
            break;
        }
        let limit = if index + 1 == MAX_PARTS {
            PART_UNITS - 100
        } else {
            PART_UNITS
        };
        let mut units = 0;
        let mut part = String::new();
        while let Some(character) = characters.peek().copied() {
            if units + character.len_utf16() > limit {
                break;
            }
            units += character.len_utf16();
            part.push(character);
            characters.next();
        }
        if index + 1 == MAX_PARTS && characters.peek().is_some() {
            part.push_str("\n… More results omitted / Показана часть результата");
        }
        result.push(part);
    }
    result
}
pub fn short(text: &str) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 190
        })
        .collect()
}
pub fn callback(data: &str) -> Option<(&str, i64)> {
    if data.len() > 64 {
        return None;
    }
    let (action, id) = data.split_once(':')?;
    if !matches!(action, "join" | "leave" | "people") || !id.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let id = id.parse().ok()?;
    (id > 0).then_some((action, id))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_parts_are_bounded_without_splitting_characters() {
        let text = "🦀Привет".repeat(700);
        let pages = parts(&text);
        assert_eq!(pages.concat(), text);
        assert!(pages.iter().all(|p| p.encode_utf16().count() <= PART_UNITS));
        assert!(parts(&"🦀".repeat(50000)).len() <= MAX_PARTS);
        assert!(parts(&"🦀".repeat(50000))
            .last()
            .expect("page")
            .contains("Показана часть"));
        assert!(short(&"🦀".repeat(200)).encode_utf16().count() <= 190);
    }
    #[test]
    fn callback_data_requires_a_supported_action_and_positive_id() {
        assert_eq!(callback("people:42"), Some(("people", 42)));
        for invalid in [
            "create:1",
            "join:0",
            "join:-1",
            "join:1:2",
            "join:",
            "join:9223372036854775808",
        ] {
            assert!(callback(invalid).is_none());
        }
        assert!(callback(&format!("join:{}", "1".repeat(65))).is_none());
    }
}
