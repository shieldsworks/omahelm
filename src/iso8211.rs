//! ISO/IEC 8211, the record format S-57 cells are written in.
//!
//! A file is one descriptive record, which names each field and the format
//! of its subfields, followed by data records. Only what the S-57 binary
//! implementation uses is read: fixed and variable text, little-endian
//! integers, and bit fields.

use std::collections::HashMap;

/// Ends a variable-length subfield.
pub const UNIT_END: u8 = 0x1f;
/// Ends a field.
pub const FIELD_END: u8 = 0x1e;

pub type Result<T> = std::result::Result<T, String>;

/// One subfield's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Text of a fixed width, or up to a unit terminator (`A`, `I`, `R`).
    Text(Option<usize>),
    /// A little-endian unsigned integer of this many bytes (`b1n`).
    Unsigned(usize),
    /// A little-endian signed integer of this many bytes (`b2n`).
    Signed(usize),
    /// A bit field of this many bytes (`B(bits)`).
    Bits(usize),
}

/// How a field's subfields are laid out, from the descriptive record.
#[derive(Debug)]
pub struct FieldDefn {
    /// The subfields repeat until the end of the field.
    pub repeating: bool,
    pub names: Vec<String>,
    pub formats: Vec<Format>,
}

#[derive(Clone, Copy, Debug)]
pub enum Value<'a> {
    Int(i64),
    Text(&'a [u8]),
    Bits(&'a [u8]),
}

impl<'a> Value<'a> {
    pub fn int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Text(t) => std::str::from_utf8(t)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0),
            Value::Bits(_) => 0,
        }
    }

    /// Text as ISO 8859-1, which is also plain ASCII.
    pub fn text(&self) -> String {
        match self {
            Value::Text(t) => t.iter().map(|&b| b as char).collect(),
            _ => String::new(),
        }
    }

    pub fn bits(&self) -> &'a [u8] {
        match self {
            Value::Bits(b) => b,
            _ => &[],
        }
    }
}

impl FieldDefn {
    /// Every repetition of the field's subfields, in order. A field that
    /// doesn't repeat has one row.
    pub fn rows<'a>(&self, mut data: &'a [u8]) -> Result<Vec<Vec<Value<'a>>>> {
        let mut rows = Vec::new();
        if self.formats.is_empty() {
            return Ok(rows);
        }
        while !data.is_empty() && data[0] != FIELD_END {
            let mut row = Vec::with_capacity(self.formats.len());
            for format in &self.formats {
                let (value, used) = value(*format, data)?;
                row.push(value);
                data = &data[used..];
            }
            rows.push(row);
            if !self.repeating {
                break;
            }
        }
        Ok(rows)
    }

    /// The position of a named subfield in a row.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
}

fn value(format: Format, data: &[u8]) -> Result<(Value<'_>, usize)> {
    let need = |n: usize| {
        if data.len() < n {
            Err(format!("subfield needs {n} bytes, {} left", data.len()))
        } else {
            Ok(())
        }
    };
    Ok(match format {
        Format::Text(Some(width)) => {
            need(width)?;
            (Value::Text(&data[..width]), width)
        }
        Format::Text(None) => {
            let end = data
                .iter()
                .position(|&b| b == UNIT_END || b == FIELD_END)
                .unwrap_or(data.len());
            // A field terminator ends the field, so it's left in place.
            let used = if data.get(end) == Some(&UNIT_END) {
                end + 1
            } else {
                end
            };
            (Value::Text(&data[..end]), used)
        }
        Format::Unsigned(n) => {
            need(n)?;
            let v = data[..n]
                .iter()
                .rev()
                .fold(0u64, |acc, &b| (acc << 8) | b as u64);
            (Value::Int(v as i64), n)
        }
        Format::Signed(n) => {
            need(n)?;
            let v = data[..n]
                .iter()
                .rev()
                .fold(0u64, |acc, &b| (acc << 8) | b as u64);
            let shift = 64 - 8 * n as u32;
            (Value::Int(((v << shift) as i64) >> shift), n)
        }
        Format::Bits(n) => {
            need(n)?;
            (Value::Bits(&data[..n]), n)
        }
    })
}

/// One field of a record, its terminator removed.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub tag: &'a str,
    pub data: &'a [u8],
}

/// An ISO 8211 file: its field definitions, then its data records in turn.
pub struct Module<'a> {
    defns: HashMap<String, FieldDefn>,
    rest: &'a [u8],
}

impl<'a> Module<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self> {
        let (leader, fields, used) = record(bytes)?;
        if leader.id != b'L' {
            return Err("not an ISO 8211 file: no descriptive record".into());
        }
        let mut defns = HashMap::new();
        for field in fields {
            if field.tag == "0000" {
                continue;
            }
            // A field this reader can't describe is one S-57 doesn't need;
            // asking for it later reports it missing.
            if let Ok(defn) = describe(field.data, leader.controls) {
                defns.insert(field.tag.to_string(), defn);
            }
        }
        Ok(Module {
            defns,
            rest: &bytes[used..],
        })
    }

    pub fn defn(&self, tag: &str) -> Result<&FieldDefn> {
        self.defns
            .get(tag)
            .ok_or_else(|| format!("no definition for field {tag}"))
    }

    /// The next data record's fields, or None at the end of the file.
    pub fn next_record(&mut self) -> Result<Option<Vec<Field<'a>>>> {
        // Some writers pad the end of a file.
        if self.rest.iter().all(|&b| b == 0 || b == b' ' || b == b'\n') {
            return Ok(None);
        }
        let (leader, fields, used) = record(self.rest)?;
        if leader.id != b'D' {
            return Err(format!(
                "unsupported data record leader {:?}",
                leader.id as char
            ));
        }
        self.rest = &self.rest[used..];
        Ok(Some(fields))
    }
}

struct Leader {
    id: u8,
    /// Field control length, in the descriptive record only.
    controls: usize,
}

fn number(bytes: &[u8]) -> Option<usize> {
    std::str::from_utf8(bytes).ok()?.trim().parse().ok()
}

fn record(bytes: &[u8]) -> Result<(Leader, Vec<Field<'_>>, usize)> {
    if bytes.len() < 24 {
        return Err("truncated record leader".into());
    }
    let length = number(&bytes[0..5]).ok_or("bad record length")?;
    let id = bytes[6];
    let controls = number(&bytes[10..12]).unwrap_or(0);
    let base = number(&bytes[12..17]).ok_or("bad field area address")?;
    let digit = |b: u8| (b as char).to_digit(10).map(|d| d as usize);
    let (Some(size_len), Some(size_pos), Some(size_tag)) =
        (digit(bytes[20]), digit(bytes[21]), digit(bytes[23]))
    else {
        return Err("bad entry map in record leader".into());
    };
    let entry = size_len + size_pos + size_tag;
    if entry == 0 || base < 25 || base > bytes.len() {
        return Err("bad record directory".into());
    }
    let directory = &bytes[24..base - 1];
    let mut fields = Vec::with_capacity(directory.len() / entry);
    let mut end = base;
    for e in directory.chunks_exact(entry) {
        let tag = std::str::from_utf8(&e[..size_tag]).map_err(|_| "bad field tag")?;
        let len = number(&e[size_tag..size_tag + size_len]).ok_or("bad field length")?;
        let pos = number(&e[size_tag + size_len..]).ok_or("bad field position")?;
        let (start, stop) = (base + pos, base + pos + len);
        if stop > bytes.len() {
            return Err(format!("field {tag} runs past the end of the file"));
        }
        let mut data = &bytes[start..stop];
        if data.last() == Some(&FIELD_END) {
            data = &data[..data.len() - 1];
        }
        fields.push(Field { tag, data });
        end = end.max(stop);
    }
    // A record too long for five digits says 0 and is measured instead.
    let length = if length == 0 { end } else { length };
    if length > bytes.len() || length < end {
        return Err("record runs past the end of the file".into());
    }
    Ok((Leader { id, controls }, fields, length))
}

fn describe(data: &[u8], controls: usize) -> Result<FieldDefn> {
    if data.len() < controls {
        return Err("truncated field description".into());
    }
    let mut parts = data[controls..].split(|&b| b == UNIT_END);
    let _name = parts.next();
    let array = String::from_utf8_lossy(parts.next().unwrap_or_default());
    let formats = String::from_utf8_lossy(parts.next().unwrap_or_default());
    let repeating = array.starts_with('*');
    let names = array
        .trim_start_matches('*')
        .split('!')
        .filter(|n| !n.is_empty())
        .map(String::from)
        .collect();
    let formats = if formats.trim().is_empty() {
        Vec::new()
    } else {
        parse_formats(formats.trim())?
    };
    Ok(FieldDefn {
        repeating,
        names,
        formats,
    })
}

/// Expands a format control string such as `(b11,b14,2b11,A(3),B(40))`.
pub fn parse_formats(s: &str) -> Result<Vec<Format>> {
    let inner = s
        .strip_prefix('(')
        .and_then(|t| t.strip_suffix(')'))
        .ok_or_else(|| format!("bad format controls {s}"))?;
    let mut out = Vec::new();
    list(inner, &mut out)?;
    Ok(out)
}

fn list(s: &str, out: &mut Vec<Format>) -> Result<()> {
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                item(s[start..i].trim(), out)?;
                start = i + 1;
            }
            _ => {}
        }
    }
    item(s[start..].trim(), out)
}

fn item(s: &str, out: &mut Vec<Format>) -> Result<()> {
    if s.is_empty() {
        return Ok(());
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    let count = if digits > 0 {
        s[..digits].parse().map_err(|_| "bad repeat count")?
    } else {
        1
    };
    let rest = &s[digits..];
    let mut group = Vec::new();
    if let Some(inner) = rest.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        list(inner, &mut group)?;
    } else {
        group.push(single(rest)?);
    }
    for _ in 0..count {
        out.extend_from_slice(&group);
    }
    Ok(())
}

fn single(s: &str) -> Result<Format> {
    let (kind, width) = match s.find('(') {
        Some(i) => {
            let w = s[i + 1..]
                .strip_suffix(')')
                .and_then(|w| w.parse::<usize>().ok())
                .ok_or_else(|| format!("bad width in {s}"))?;
            (&s[..i], Some(w))
        }
        None => (s, None),
    };
    let bad = || format!("unsupported subfield format {s}");
    match kind {
        "A" | "I" | "R" => Ok(Format::Text(width)),
        "B" => Ok(Format::Bits(width.ok_or_else(bad)?.div_ceil(8))),
        _ => {
            let b = kind.as_bytes();
            if b.len() != 3 || b[0] != b'b' {
                return Err(bad());
            }
            let n = (b[2] as char).to_digit(10).ok_or_else(bad)? as usize;
            if !matches!(n, 1 | 2 | 4 | 8) {
                return Err(bad());
            }
            match b[1] {
                b'1' => Ok(Format::Unsigned(n)),
                b'2' => Ok(Format::Signed(n)),
                _ => Err(bad()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_expand_repeats_and_groups() {
        use Format::*;
        assert_eq!(
            parse_formats("(b11,b14,2b11,A(3),B(40),A)").unwrap(),
            vec![
                Unsigned(1),
                Unsigned(4),
                Unsigned(1),
                Unsigned(1),
                Text(Some(3)),
                Bits(5),
                Text(None)
            ]
        );
        assert_eq!(
            parse_formats("(2(b24,b12))").unwrap(),
            vec![Signed(4), Unsigned(2), Signed(4), Unsigned(2)]
        );
        assert!(parse_formats("(b35)").is_err());
    }

    #[test]
    fn values_decode_little_endian() {
        let (v, n) = value(Format::Signed(4), &[0xff, 0xff, 0xff, 0xff]).unwrap();
        assert_eq!((v.int(), n), (-1, 4));
        let (v, _) = value(Format::Unsigned(2), &[0x2a, 0x01]).unwrap();
        assert_eq!(v.int(), 298);
        let (v, n) = value(Format::Text(None), b"Fl G\x1fnext").unwrap();
        assert_eq!((v.text().as_str(), n), ("Fl G", 5));
        assert!(value(Format::Unsigned(4), &[1, 2]).is_err());
    }

    #[test]
    fn repeating_rows() {
        let defn = FieldDefn {
            repeating: true,
            names: vec!["ATTL".into(), "ATVL".into()],
            formats: vec![Format::Unsigned(2), Format::Text(None)],
        };
        let rows = defn.rows(b"\x74\x00Alcatraz\x1f\x4b\x00\x1f").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].int(), 116);
        assert_eq!(rows[0][1].text(), "Alcatraz");
        assert_eq!(rows[1][1].text(), "");
    }
}
