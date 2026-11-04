//! Finite URL patterns: literals, character classes/ranges and exact repetitions.
use std::collections::HashSet;
use actix_web::{error, web, Error, HttpResponse};
use rand::Rng;
use serde::Deserialize;
use crate::{args::ARGS, pasta::Pasta, AppState};

const RESERVED: &[&str] = &[
    "list", "admin", "auth_admin", "upload", "incorrect", "guide", "static",
    "p", "u", "url", "raw", "file", "archive", "qr", "auth", "edit", "remove",
];
pub const FULL: &str = "该短链接格式的可用编号已耗尽，请选择其他格式或删除部分分享后重试";

pub struct Pattern {
    positions: Vec<Vec<u8>>,
    pub capacity: u64,
}

fn safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

impl Pattern {
    pub fn parse(format: &str) -> Result<Self, &'static str> {
        let source = match format {
            "lower5" => "[abcdefghjkmnpqrstuvwxyz]{5}",
            "lower6" => "[abcdefghjkmnpqrstuvwxyz]{6}",
            "digits6" => "[0-9]{6}",
            "mixed6" => "[abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789]{6}",
            other => other,
        };
        if source.is_empty() || source.len() > 256 || !source.is_ascii() {
            return Err("格式长度须为 1–256 个 ASCII 字符");
        }
        let bytes = source.as_bytes();
        let mut i = 0;
        let mut positions = Vec::new();
        let mut capacity = 1u64;
        while i < bytes.len() {
            let mut choices = Vec::new();
            if bytes[i] == b'[' {
                i += 1;
                while i < bytes.len() && bytes[i] != b']' {
                    let start = bytes[i];
                    if !safe(start) { return Err("字符集仅支持英文字母、数字、_ 和 -"); }
                    if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] != b']' {
                        let end = bytes[i + 2];
                        if start > end || !(start..=end).all(safe) {
                            return Err("字符范围无效，请使用 a-z、A-Z 或 0-9 等范围");
                        }
                        choices.extend(start..=end);
                        i += 3;
                    } else {
                        choices.push(start);
                        i += 1;
                    }
                }
                if i == bytes.len() || choices.is_empty() { return Err("字符集不能为空，且必须以 ] 结尾"); }
                i += 1;
            } else {
                if !safe(bytes[i]) { return Err("仅支持字母数字、_、-、[字符集] 和 {次数}，不支持 *、+、| 等语法"); }
                choices.push(bytes[i]);
                i += 1;
            }
            choices.sort_unstable();
            choices.dedup();
            let mut repeat = 1;
            if i < bytes.len() && bytes[i] == b'{' {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                if i == bytes.len() || bytes[i] != b'}' { return Err("重复次数请写成 {5}，不支持可变长度"); }
                repeat = source[start..i].parse::<usize>().map_err(|_| "重复次数无效")?;
                if repeat == 0 || repeat > 32 { return Err("重复次数须为 1–32"); }
                i += 1;
            }
            if positions.len() + repeat > 32 { return Err("生成的短编号最多 32 位"); }
            for _ in 0..repeat {
                capacity = capacity.checked_mul(choices.len() as u64).ok_or("格式容量过大，请缩短长度或字符集")?;
                positions.push(choices.clone());
            }
        }
        Ok(Self { positions, capacity })
    }

    pub fn at(&self, mut index: u64) -> String {
        let mut bytes = vec![0; self.positions.len()];
        for (i, choices) in self.positions.iter().enumerate().rev() {
            bytes[i] = choices[(index % choices.len() as u64) as usize];
            index /= choices.len() as u64;
        }
        String::from_utf8(bytes).unwrap()
    }

    fn contains(&self, slug: &str) -> bool {
        slug.len() == self.positions.len() && slug.bytes().zip(&self.positions)
            .all(|(byte, choices)| choices.contains(&byte))
    }

    fn occupied(&self, pastas: &[Pasta]) -> HashSet<String> {
        pastas.iter().map(Pasta::id_as_animals)
            .chain(RESERVED.iter().map(|s| s.to_string()))
            .filter(|slug| self.contains(slug)).collect()
    }

    pub fn remaining(&self, pastas: &[Pasta]) -> u64 {
        self.capacity - self.occupied(pastas).len() as u64
    }

    pub fn allocate(&self, pastas: &[Pasta]) -> Result<String, &'static str> {
        let used = self.occupied(pastas);
        if used.len() as u64 == self.capacity { return Err(FULL); }
        let start = rand::thread_rng().gen_range(0..self.capacity);
        // At most N occupied slots can follow a random start; no unbounded retries near capacity.
        for offset in 0..=used.len() as u64 {
            let index = ((start as u128 + offset as u128) % self.capacity as u128) as u64;
            let slug = self.at(index);
            if !used.contains(&slug) { return Ok(slug); }
        }
        unreachable!()
    }
}

// Exact stored/canonical names only. Invalid input never resolves to upload zero.
pub fn resolve_id(slug: &str, pastas: &[Pasta]) -> u64 {
    pastas.iter().find(|p| p.id_as_animals() == slug).map_or(u64::MAX, |p| p.id)
}

#[derive(Deserialize)]
pub struct FormatInput {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub custom: String,
}

impl FormatInput {
    pub fn pattern(&self) -> Result<Option<Pattern>, &'static str> {
        let format = if self.format.is_empty() { &ARGS.short_link_format } else { &self.format };
        if format == "legacy" { return Ok(None); }
        Pattern::parse(if format == "custom" { &self.custom } else { format }).map(Some)
    }
}

pub async fn availability(data: web::Data<AppState>, input: web::Json<FormatInput>) -> Result<HttpResponse, Error> {
    let pattern = input.pattern().map_err(error::ErrorBadRequest)?;
    let mut pastas = data.pastas.lock().unwrap();
    crate::util::misc::remove_expired(&mut pastas);
    let result = if let Some(pattern) = pattern {
        let remaining = pattern.remaining(&pastas);
        serde_json::json!({"available": remaining > 0, "capacity": pattern.capacity.to_string(),
            "remaining": remaining.to_string(), "example": pattern.at(rand::thread_rng().gen_range(0..pattern.capacity)), "message": FULL})
    } else {
        serde_json::json!({"available": true, "example": "aB3xYz"})
    };
    Ok(HttpResponse::Ok().insert_header(("Cache-Control", "no-store")).json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_patterns_validate_and_enumerate() {
        let pattern = Pattern::parse("img-[ab]{2}[2-3]").unwrap();
        assert_eq!(pattern.capacity, 8);
        let slugs: HashSet<_> = (0..8).map(|i| pattern.at(i)).collect();
        assert_eq!(slugs.len(), 8);
        assert!(slugs.contains("img-aa2") && slugs.contains("img-bb3"));
        assert_eq!(Pattern::parse("lower5").unwrap().capacity, 23u64.pow(5));
        assert_eq!(Pattern::parse("list").unwrap().remaining(&[]), 0);
        assert_eq!(Pattern::parse("list").unwrap().allocate(&[]), Err(FULL));
        for invalid in ["", "[]", "[", "[z-a]", "[a-z]*", "[ab]{0}", "a{33}", "a{1,2}", "a{", "中文", "../x", "[a-z]{32}", "[a^]"] {
            assert!(Pattern::parse(invalid).is_err(), "{invalid}");
        }
    }
}
