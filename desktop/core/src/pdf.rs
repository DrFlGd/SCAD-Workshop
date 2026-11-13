//! Text from PDF files, enough to read the page PDFs that model sites put in their downloads
//! (milestone 5, 0.4.16): Printables' "PDF of the model page" carries the creator, license,
//! dates, summary and tags. Not a general PDF reader: it finds objects by scanning the file (no
//! cross-reference table needed, so damaged files still give what they can), unpacks object
//! streams, decodes Flate content streams and maps text through each font's ToUnicode map. Each
//! piece of text keeps where it sits on the page, its size and whether it's white (Printables'
//! tags are white on grey), and lines are rebuilt from those positions rather than the order the
//! file draws them in.

use anyhow::{bail, Result};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

/// Files bigger than this aren't read (a page PDF is a few MB at most).
const MAX_FILE: usize = 64 << 20;
/// A stream that unpacks to more than this is cut short.
const MAX_STREAM: u64 = 64 << 20;

// ------------------------------------------------------------------ objects

#[derive(Clone, Debug, PartialEq)]
enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<Obj>),
    Dict(BTreeMap<String, Obj>),
    Ref(u32),
    /// A keyword: an operator in a content stream (`Tj`, `BT`) or CMap (`beginbfchar`).
    Op(String),
}

impl Obj {
    fn num(&self) -> Option<f64> {
        if let Obj::Num(n) = self { Some(*n) } else { None }
    }
    fn name(&self) -> Option<&str> {
        if let Obj::Name(n) = self { Some(n) } else { None }
    }
    fn dict(&self) -> Option<&BTreeMap<String, Obj>> {
        if let Obj::Dict(d) = self { Some(d) } else { None }
    }
}

/// Reads PDF objects (and content stream tokens) from bytes.
struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

fn is_white(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}
fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

impl<'a> Lexer<'a> {
    fn new(b: &'a [u8]) -> Self {
        Lexer { b, i: 0 }
    }

    fn skip_space(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if is_white(c) {
                self.i += 1;
            } else if c == b'%' {
                while self.i < self.b.len() && self.b[self.i] != b'\n' && self.b[self.i] != b'\r' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let s = self.i;
        while self.i < self.b.len() && !is_white(self.b[self.i]) && !is_delim(self.b[self.i]) {
            self.i += 1;
        }
        &self.b[s..self.i]
    }

    /// The next object; `None` at the end. `R` references are only joined up when `refs`.
    fn next(&mut self, refs: bool) -> Option<Obj> {
        self.skip_space();
        let c = *self.b.get(self.i)?;
        Some(match c {
            b'/' => {
                self.i += 1;
                let w = self.word();
                let mut out = Vec::with_capacity(w.len());
                let mut k = 0;
                while k < w.len() {
                    if w[k] == b'#' && k + 2 < w.len() {
                        if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&w[k + 1..k + 3]).unwrap_or(""), 16) {
                            out.push(v);
                            k += 3;
                            continue;
                        }
                    }
                    out.push(w[k]);
                    k += 1;
                }
                Obj::Name(String::from_utf8_lossy(&out).into_owned())
            }
            b'(' => Obj::Str(self.literal()),
            b'<' if self.b.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                let mut d = BTreeMap::new();
                loop {
                    self.skip_space();
                    if self.i >= self.b.len() {
                        break;
                    }
                    if self.b[self.i..].starts_with(b">>") {
                        self.i += 2;
                        break;
                    }
                    match self.next(refs) {
                        Some(Obj::Name(k)) => {
                            let v = self.next(refs).unwrap_or(Obj::Null);
                            d.insert(k, v);
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                Obj::Dict(d)
            }
            b'<' => {
                self.i += 1;
                let s = self.i;
                while self.i < self.b.len() && self.b[self.i] != b'>' {
                    self.i += 1;
                }
                let hex: Vec<u8> = self.b[s..self.i].iter().copied().filter(|c| c.is_ascii_hexdigit()).collect();
                self.i += 1;
                let mut out = Vec::with_capacity(hex.len() / 2 + 1);
                for pair in hex.chunks(2) {
                    let hi = (pair[0] as char).to_digit(16).unwrap_or(0) as u8;
                    let lo = pair.get(1).map(|c| (*c as char).to_digit(16).unwrap_or(0) as u8).unwrap_or(0);
                    out.push(hi << 4 | lo);
                }
                Obj::Str(out)
            }
            b'[' => {
                self.i += 1;
                let mut a = vec![];
                loop {
                    self.skip_space();
                    if self.i >= self.b.len() {
                        break;
                    }
                    if self.b[self.i] == b']' {
                        self.i += 1;
                        break;
                    }
                    match self.next(refs) {
                        Some(o) => a.push(o),
                        None => break,
                    }
                }
                Obj::Arr(a)
            }
            b']' | b'>' | b')' | b'{' | b'}' => {
                self.i += 1;
                Obj::Op((c as char).to_string())
            }
            _ => {
                let w = self.word();
                if w.is_empty() {
                    self.i += 1;
                    return Some(Obj::Null);
                }
                if let Some(n) = std::str::from_utf8(w).ok().and_then(|s| s.parse::<f64>().ok()).filter(|_| w[0] != b'i' && w[0] != b'n' && w[0] != b'N' && w[0] != b'I') {
                    // "12 0 R"
                    if refs && w.iter().all(u8::is_ascii_digit) {
                        let save = self.i;
                        self.skip_space();
                        let g = self.word();
                        if !g.is_empty() && g.iter().all(u8::is_ascii_digit) {
                            self.skip_space();
                            if self.b.get(self.i) == Some(&b'R') && self.b.get(self.i + 1).is_none_or(|c| is_white(*c) || is_delim(*c)) {
                                self.i += 1;
                                return Some(Obj::Ref(n as u32));
                            }
                        }
                        self.i = save;
                    }
                    return Some(Obj::Num(n));
                }
                match w {
                    b"true" => Obj::Bool(true),
                    b"false" => Obj::Bool(false),
                    b"null" => Obj::Null,
                    _ => Obj::Op(String::from_utf8_lossy(w).into_owned()),
                }
            }
        })
    }

    fn literal(&mut self) -> Vec<u8> {
        self.i += 1;
        let mut out = vec![];
        let mut depth = 1;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c);
                }
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else { break };
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\r' => {
                            if self.b.get(self.i) == Some(&b'\n') {
                                self.i += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.b.get(self.i) {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        _ => out.push(e),
                    }
                }
                _ => out.push(c),
            }
        }
        out
    }
}

/// A PDF's objects by number: each one's value and, for a stream, its raw (still encoded) data.
struct Objects {
    objs: HashMap<u32, (Obj, Option<Vec<u8>>)>,
}

impl Objects {
    fn scan(data: &[u8]) -> Objects {
        let mut objs: HashMap<u32, (Obj, Option<Vec<u8>>)> = HashMap::new();
        let header = regex::bytes::Regex::new(r"(?:^|[\r\n\s])(\d+)\s+(\d+)\s+obj\b").unwrap();
        let mut at = 0;
        while let Some(m) = header.captures_at(data, at) {
            let whole = m.get(0).unwrap();
            let num: u32 = std::str::from_utf8(&m[1]).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut lx = Lexer::new(data);
            lx.i = whole.end();
            let value = lx.next(true).unwrap_or(Obj::Null);
            lx.skip_space();
            let mut stream = None;
            if data[lx.i..].starts_with(b"stream") {
                let mut s = lx.i + 6;
                if data.get(s) == Some(&b'\r') {
                    s += 1;
                }
                if data.get(s) == Some(&b'\n') {
                    s += 1;
                }
                let len = value.dict().and_then(|d| d.get("Length")).and_then(Obj::num).map(|n| n as usize);
                // the stated length when "endstream" follows it, otherwise up to "endstream"
                let fits = |n: usize| s + n <= data.len() && find(&data[s + n..data.len().min(s + n + 20)], b"endstream", 0).is_some();
                let end = match len {
                    Some(n) if fits(n) => s + n,
                    _ => find(data, b"endstream", s).unwrap_or(data.len()),
                };
                stream = Some(data[s..end].to_vec());
                lx.i = end;
            }
            if num > 0 {
                objs.insert(num, (value, stream));
            }
            at = lx.i.max(whole.end());
        }
        let mut me = Objects { objs };
        // objects packed in object streams (PDF 1.5); ones written out on their own win
        let packed: Vec<u32> = me.objs.iter().filter(|(_, (v, s))| s.is_some() && v.dict().and_then(|d| d.get("Type")).and_then(Obj::name) == Some("ObjStm")).map(|(n, _)| *n).collect();
        for n in packed {
            let Some(data) = me.stream(n) else { continue };
            let d = me.objs[&n].0.dict().cloned().unwrap_or_default();
            let count = d.get("N").and_then(Obj::num).unwrap_or(0.0) as usize;
            let first = d.get("First").and_then(Obj::num).unwrap_or(0.0) as usize;
            let mut lx = Lexer::new(&data);
            let mut pairs = vec![];
            for _ in 0..count {
                match (lx.next(false).and_then(|o| o.num()), lx.next(false).and_then(|o| o.num())) {
                    (Some(a), Some(b)) => pairs.push((a as u32, b as usize)),
                    _ => break,
                }
            }
            for (num, off) in pairs {
                if me.objs.contains_key(&num) || first + off >= data.len() {
                    continue;
                }
                let mut lx = Lexer::new(&data);
                lx.i = first + off;
                if let Some(v) = lx.next(true) {
                    me.objs.insert(num, (v, None));
                }
            }
        }
        me
    }

    fn get(&self, o: &Obj) -> Obj {
        let mut o = o.clone();
        for _ in 0..8 {
            match o {
                Obj::Ref(n) => o = self.objs.get(&n).map(|x| x.0.clone()).unwrap_or(Obj::Null),
                _ => break,
            }
        }
        o
    }

    fn get_dict(&self, o: Option<&Obj>) -> BTreeMap<String, Obj> {
        o.map(|o| self.get(o)).and_then(|o| if let Obj::Dict(d) = o { Some(d) } else { None }).unwrap_or_default()
    }

    /// A stream's data, unpacked (Flate only; other filters give `None`).
    fn stream(&self, n: u32) -> Option<Vec<u8>> {
        let (v, raw) = self.objs.get(&n)?;
        let raw = raw.as_ref()?;
        let d = v.dict()?;
        let filters: Vec<String> = match d.get("Filter").map(|f| self.get(f)) {
            None | Some(Obj::Null) => vec![],
            Some(Obj::Name(f)) => vec![f],
            Some(Obj::Arr(a)) => a.iter().filter_map(|f| f.name().map(String::from)).collect(),
            _ => return None,
        };
        let mut data = raw.clone();
        for f in filters {
            if f != "FlateDecode" && f != "Fl" {
                return None;
            }
            let mut out = vec![];
            // a damaged stream still gives what came before the damage
            let _ = flate2::read::ZlibDecoder::new(&data[..]).take(MAX_STREAM).read_to_end(&mut out);
            if out.is_empty() {
                return None;
            }
            data = out;
        }
        Some(data)
    }

    fn stream_of(&self, o: &Obj) -> Option<Vec<u8>> {
        match o {
            Obj::Ref(n) => self.stream(*n),
            _ => None,
        }
    }
}

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    h.get(from..)?.windows(n.len()).position(|w| w == n).map(|p| p + from)
}

/// A PDF text string: UTF-16 with a byte-order mark, or PDFDocEncoding (taken as Latin-1).
fn text_string(b: &[u8]) -> String {
    if b.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = b[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        String::from_utf16_lossy(&units)
    } else if b.starts_with(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(&b[3..]).into_owned()
    } else {
        b.iter().map(|c| *c as char).collect()
    }
}

// ------------------------------------------------------------------ text

/// One piece of text as drawn: where it starts (x right, y down the page from the top), its size
/// and whether it's drawn in white.
#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub white: bool,
}

/// Runs at the same height, left to right.
#[derive(Clone, Debug)]
pub struct Line {
    pub runs: Vec<Run>,
}

impl Line {
    pub fn text(&self) -> String {
        let mut s = String::new();
        for r in &self.runs {
            let t = r.text.trim();
            if t.is_empty() {
                continue;
            }
            if !s.is_empty() && !t.starts_with(['.', ',', ';', ':', '!', '?', ')']) {
                s.push(' ');
            }
            s.push_str(t);
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    pub fn x(&self) -> f64 {
        self.runs.first().map(|r| r.x).unwrap_or(0.0)
    }
    pub fn y(&self) -> f64 {
        self.runs.first().map(|r| r.y).unwrap_or(0.0)
    }
    pub fn size(&self) -> f64 {
        self.runs.iter().map(|r| r.size).fold(0.0, f64::max)
    }
}

/// What was read from a PDF.
#[derive(Debug, Default)]
pub struct Doc {
    /// Each page's lines, top to bottom.
    pub pages: Vec<Vec<Line>>,
    /// Every link's address, in the order they appear in the file.
    pub links: Vec<String>,
    /// The document's own title and the program that made it (its Info dictionary).
    pub title: Option<String>,
    pub producer: Option<String>,
    /// The bookmarks' top-level titles.
    pub outline: Vec<String>,
}

struct Font {
    two_byte: bool,
    map: HashMap<u32, String>,
}

impl Font {
    fn decode(&self, b: &[u8]) -> String {
        let mut s = String::new();
        if self.two_byte {
            for c in b.chunks(2) {
                let code = (c[0] as u32) << 8 | *c.get(1).unwrap_or(&0) as u32;
                match self.map.get(&code) {
                    Some(t) => s.push_str(t),
                    None => s.push('\u{fffd}'),
                }
            }
        } else {
            for &c in b {
                match self.map.get(&(c as u32)) {
                    Some(t) => s.push_str(t),
                    None => s.push(c as char), // standard fonts: near enough Latin-1
                }
            }
        }
        s
    }
}

/// A ToUnicode CMap: character code to text.
fn parse_cmap(data: &[u8]) -> HashMap<u32, String> {
    let code = |b: &[u8]| b.iter().fold(0u32, |a, c| a << 8 | *c as u32);
    let utf16 = |b: &[u8]| String::from_utf16_lossy(&b.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>());
    let mut map = HashMap::new();
    let mut lx = Lexer::new(data);
    let mut toks = vec![];
    while let Some(t) = lx.next(false) {
        toks.push(t);
    }
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Obj::Op(op) if op == "beginbfchar" => {
                i += 1;
                while i + 1 < toks.len() {
                    match (&toks[i], &toks[i + 1]) {
                        (Obj::Str(a), Obj::Str(b)) => {
                            map.insert(code(a), utf16(b));
                            i += 2;
                        }
                        _ => break,
                    }
                }
            }
            Obj::Op(op) if op == "beginbfrange" => {
                i += 1;
                while i + 2 < toks.len() {
                    match (&toks[i], &toks[i + 1], &toks[i + 2]) {
                        (Obj::Str(a), Obj::Str(b), Obj::Str(dst)) => {
                            let (lo, hi) = (code(a), code(b).min(code(a) + 0xffff));
                            let units: Vec<u16> = dst.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
                            for (k, c) in (lo..=hi).enumerate() {
                                let mut u = units.clone();
                                if let Some(last) = u.last_mut() {
                                    *last = last.wrapping_add(k as u16);
                                }
                                map.insert(c, String::from_utf16_lossy(&u));
                            }
                            i += 3;
                        }
                        (Obj::Str(a), Obj::Str(b), Obj::Arr(list)) => {
                            for (k, c) in (code(a)..=code(b)).enumerate() {
                                if let Some(Obj::Str(d)) = list.get(k) {
                                    map.insert(c, utf16(d));
                                }
                            }
                            i += 3;
                        }
                        _ => break,
                    }
                }
            }
            _ => i += 1,
        }
    }
    map
}

#[derive(Clone, Copy)]
struct M([f64; 6]);

impl M {
    const ID: M = M([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn from(v: &[Obj]) -> Option<M> {
        let n: Vec<f64> = v.iter().filter_map(Obj::num).collect();
        (n.len() == 6).then(|| M([n[0], n[1], n[2], n[3], n[4], n[5]]))
    }
    /// self, then o
    fn then(self, o: M) -> M {
        let [a, b, c, d, e, f] = self.0;
        let [p, q, r, s, t, u] = o.0;
        M([a * p + b * r, a * q + b * s, c * p + d * r, c * q + d * s, e * p + f * r + t, e * q + f * s + u])
    }
    fn translate(x: f64, y: f64) -> M {
        M([1.0, 0.0, 0.0, 1.0, x, y])
    }
}

/// Read a PDF's text, links, title and bookmarks.
pub fn read(data: &[u8]) -> Result<Doc> {
    if data.len() > MAX_FILE {
        bail!("The PDF is too big to read ({} MB).", data.len() >> 20);
    }
    if !data.starts_with(b"%PDF") && find(&data[..data.len().min(1024)], b"%PDF", 0).is_none() {
        bail!("Not a PDF file.");
    }
    let objs = Objects::scan(data);
    let mut doc = Doc::default();
    let mut nums: Vec<&u32> = objs.objs.keys().collect();
    nums.sort();
    let mut catalog = None;
    for n in nums {
        let v = &objs.objs[n].0;
        let Some(d) = v.dict() else { continue };
        if d.get("Type").and_then(Obj::name) == Some("Catalog") {
            catalog = Some(d.clone());
        }
        if let Some(Obj::Str(u)) = d.get("URI").map(|u| objs.get(u)) {
            doc.links.push(text_string(&u));
        }
        if let Some(Obj::Dict(a)) = d.get("A").map(|a| objs.get(a)) {
            if let Some(Obj::Str(u)) = a.get("URI").map(|u| objs.get(u)) {
                let u = text_string(&u);
                if doc.links.last() != Some(&u) || d.get("URI").is_none() {
                    doc.links.push(u);
                }
            }
        }
        if d.contains_key("Producer") && doc.producer.is_none() {
            if let Obj::Str(p) = objs.get(&d["Producer"]) {
                doc.producer = Some(text_string(&p));
            }
            if let Some(Obj::Str(t)) = d.get("Title").map(|t| objs.get(t)) {
                doc.title = Some(text_string(&t));
            }
        }
    }
    let Some(catalog) = catalog else { bail!("The PDF has no pages that can be read.") };
    // bookmarks
    let outlines = objs.get_dict(catalog.get("Outlines"));
    let mut item = outlines.get("First").cloned();
    let mut guard = 0;
    while let (Some(it), true) = (item.clone(), guard < 1000) {
        guard += 1;
        let d = objs.get_dict(Some(&it));
        if let Some(Obj::Str(t)) = d.get("Title").map(|t| objs.get(t)) {
            doc.outline.push(text_string(&t));
        }
        item = d.get("Next").cloned();
    }
    // pages, in order, with the resources they inherit
    let mut pages = vec![];
    fn walk(objs: &Objects, node: &Obj, res: Option<Obj>, out: &mut Vec<(BTreeMap<String, Obj>, Option<Obj>)>, depth: usize) {
        if depth > 32 || out.len() > 2000 {
            return;
        }
        let d = objs.get_dict(Some(node));
        let res = d.get("Resources").cloned().or(res);
        match d.get("Kids").map(|k| objs.get(k)) {
            Some(Obj::Arr(kids)) => {
                for k in kids {
                    walk(objs, &k, res.clone(), out, depth + 1);
                }
            }
            _ => out.push((d, res)),
        }
    }
    if let Some(p) = catalog.get("Pages") {
        walk(&objs, p, None, &mut pages, 0);
    }
    let mut fonts: HashMap<String, Font> = HashMap::new(); // by the font object's number
    for (page, res) in pages {
        let res = objs.get_dict(res.as_ref());
        let font_dict = objs.get_dict(res.get("Font"));
        let mut by_name: HashMap<String, String> = HashMap::new();
        for (name, f) in &font_dict {
            let key = match f { Obj::Ref(n) => n.to_string(), _ => format!("{:p}{name}", &font_dict) };
            if !fonts.contains_key(&key) {
                let fd = objs.get_dict(Some(f));
                let two_byte = fd.get("Subtype").and_then(Obj::name) == Some("Type0");
                let map = fd.get("ToUnicode").and_then(|t| objs.stream_of(t)).map(|s| parse_cmap(&s)).unwrap_or_default();
                fonts.insert(key.clone(), Font { two_byte, map });
            }
            by_name.insert(name.clone(), key);
        }
        let mut content = vec![];
        match page.get("Contents").map(|c| (c.clone(), objs.get(c))) {
            Some((_, Obj::Arr(parts))) => {
                for p in parts {
                    if let Some(s) = objs.stream_of(&p) {
                        content.extend_from_slice(&s);
                        content.push(b'\n');
                    }
                }
            }
            Some((r, _)) => {
                if let Some(s) = objs.stream_of(&r) {
                    content = s;
                }
            }
            None => {}
        }
        let height = page.get("MediaBox").map(|m| objs.get(m)).and_then(|m| if let Obj::Arr(a) = m { a.get(3).and_then(Obj::num) } else { None }).unwrap_or(842.0);
        let runs = page_runs(&content, &fonts, &by_name, height);
        doc.pages.push(lines(runs));
    }
    Ok(doc)
}

/// The text a content stream draws.
fn page_runs(content: &[u8], fonts: &HashMap<String, Font>, by_name: &HashMap<String, String>, height: f64) -> Vec<Run> {
    #[derive(Clone, Copy)]
    struct Gs {
        ctm: M,
        white: bool,
    }
    let mut gs = Gs { ctm: M::ID, white: false };
    let mut stack: Vec<Gs> = vec![];
    let (mut tm, mut tlm) = (M::ID, M::ID);
    let (mut font, mut size, mut leading) = (None::<&Font>, 0.0, 0.0);
    let mut runs = vec![];
    let mut args: Vec<Obj> = vec![];
    let mut lx = Lexer::new(content);
    let num = |a: &[Obj], k: usize| a.get(k).and_then(Obj::num).unwrap_or(0.0);
    while let Some(t) = lx.next(false) {
        let Obj::Op(op) = t else {
            args.push(t);
            if args.len() > 64 {
                args.remove(0);
            }
            continue;
        };
        let a = std::mem::take(&mut args);
        let show = |s: &[u8], tm: &M, font: Option<&Font>, gs: &Gs, runs: &mut Vec<Run>, append: bool| {
            let Some(f) = font else { return };
            let text = f.decode(s);
            if text.is_empty() {
                return;
            }
            let m = tm.then(gs.ctm);
            let scale = (m.0[2] * m.0[2] + m.0[3] * m.0[3]).sqrt();
            if append {
                if let Some(last) = runs.last_mut() {
                    last.text.push_str(&text);
                    return;
                }
            }
            runs.push(Run { text, x: m.0[4], y: height - m.0[5], size: size * scale, white: gs.white });
        };
        match op.as_str() {
            "q" => stack.push(gs),
            "Q" => gs = stack.pop().unwrap_or(gs),
            "cm" => {
                if let Some(m) = M::from(&a) {
                    gs.ctm = m.then(gs.ctm);
                }
            }
            "rg" => gs.white = a.len() == 3 && (0..3).all(|k| num(&a, k) > 0.9),
            "g" => gs.white = num(&a, 0) > 0.9,
            "k" => gs.white = a.len() == 4 && (0..4).all(|k| num(&a, k) < 0.1),
            "sc" | "scn" => gs.white = !a.is_empty() && a.iter().all(|v| v.num().is_some_and(|n| n > 0.9)) && a.len() != 4,
            "BT" => {
                tm = M::ID;
                tlm = M::ID;
            }
            "Tf" => {
                font = a.first().and_then(Obj::name).and_then(|n| by_name.get(n)).and_then(|k| fonts.get(k));
                size = num(&a, 1);
            }
            "TL" => leading = num(&a, 0),
            "Tm" => {
                if let Some(m) = M::from(&a) {
                    tm = m;
                    tlm = m;
                }
            }
            "Td" | "TD" => {
                if op == "TD" {
                    leading = -num(&a, 1);
                }
                tlm = M::translate(num(&a, 0), num(&a, 1)).then(tlm);
                tm = tlm;
            }
            "T*" => {
                tlm = M::translate(0.0, -leading).then(tlm);
                tm = tlm;
            }
            "Tj" | "'" | "\"" => {
                if op != "Tj" {
                    tlm = M::translate(0.0, -leading).then(tlm);
                    tm = tlm;
                }
                if let Some(Obj::Str(s)) = a.last() {
                    show(s, &tm, font, &gs, &mut runs, false);
                }
            }
            "TJ" => {
                if let Some(Obj::Arr(parts)) = a.last() {
                    let mut first = true;
                    for p in parts {
                        match p {
                            Obj::Str(s) => {
                                show(s, &tm, font, &gs, &mut runs, !first);
                                first = false;
                            }
                            // a gap wider than a fifth of the text size reads as a space
                            Obj::Num(n) if *n < -200.0 && !first => {
                                if let Some(r) = runs.last_mut() {
                                    r.text.push(' ');
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    runs
}

/// Runs into lines: top to bottom, then left to right; runs whose heights are within a third of
/// the text size are one line.
fn lines(mut runs: Vec<Run>) -> Vec<Line> {
    runs.retain(|r| !r.text.trim().is_empty());
    for r in &mut runs {
        // icons from symbol fonts (Unicode's private use area)
        r.text = r.text.chars().filter(|c| !('\u{e000}'..='\u{f8ff}').contains(c) && *c != '\u{fffd}').collect();
    }
    runs.retain(|r| !r.text.trim().is_empty());
    runs.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut out: Vec<Line> = vec![];
    for r in runs {
        match out.last_mut() {
            Some(l) if (r.y - l.y()).abs() < (l.size().min(r.size) / 3.0).max(1.0) => l.runs.push(r),
            _ => out.push(Line { runs: vec![r] }),
        }
    }
    for l in &mut out {
        l.runs.sort_by(|a, b| a.x.total_cmp(&b.x));
    }
    out
}

// ------------------------------------------------------------------ Printables

/// What a Printables page PDF says about its model.
#[derive(Debug, Default, PartialEq)]
pub struct Details {
    pub title: String,
    /// The model's page.
    pub url: Option<String>,
    pub author: Option<String>,
    pub author_url: Option<String>,
    /// The page's one-line summary.
    pub summary: String,
    /// The rest of the page's description, as Markdown-ish text (paragraphs and "- " lists).
    pub description: String,
    /// Printables' own category ("Hobby & Makers > Organizers").
    pub category: Option<String>,
    pub tags: Vec<String>,
    /// ISO dates.
    pub published: Option<String>,
    pub updated: Option<String>,
    /// SPDX id when the license is one with an id ("CC-BY-4.0"), and the license as the page words it.
    pub license: Option<String>,
    pub license_text: String,
    /// "Label: value" lines of the Print settings section.
    pub print_settings: Vec<(String, String)>,
}

/// The details in a Printables page PDF; `None` when the PDF isn't one.
pub fn printables(doc: &Doc) -> Option<Details> {
    let page_re = regex::Regex::new(r"^https?://(?:www\.)?printables\.com/(?:[a-z]{2}/)?model/(\d+[^?#\s]*)").unwrap();
    let url = doc.links.iter().find_map(|l| page_re.captures(l).map(|c| format!("https://www.printables.com/model/{}", &c[1])));
    let made_by_printables = doc.title.as_deref() == Some("Printables PDF");
    if url.is_none() && !made_by_printables {
        return None;
    }
    let mut d = Details { url, ..Default::default() };
    let author_re = regex::Regex::new(r"^https?://(?:www\.)?printables\.com/(?:[a-z]{2}/)?@([^/?#\s]+)/?$").unwrap();
    d.author_url = doc.links.iter().find_map(|l| author_re.captures(l).map(|c| format!("https://www.printables.com/@{}", &c[1])));

    let all: Vec<(usize, &Line)> = doc.pages.iter().enumerate().flat_map(|(i, p)| p.iter().map(move |l| (i, l))).collect();
    if all.is_empty() {
        return Some(d);
    }
    // headings: the size of the section names Printables uses (else the biggest text at the left
    // edge), at the left edge
    let margin = all.iter().map(|(_, l)| l.x()).fold(f64::MAX, f64::min);
    let at_margin = |l: &Line| (l.x() - margin).abs() < 2.0;
    let known = ["summary", "description", "print settings", "model files", "other files", "license"];
    let heading_size = all.iter().find(|(_, l)| at_margin(l) && known.contains(&l.text().to_lowercase().as_str())).map(|(_, l)| l.size())
        .unwrap_or_else(|| all.iter().filter(|(_, l)| at_margin(l)).map(|(_, l)| l.size()).fold(0.0, f64::max));
    let is_heading = |l: &Line| at_margin(l) && (l.size() - heading_size).abs() < 0.5;
    let first_heading = all.iter().position(|(_, l)| is_heading(l)).unwrap_or(all.len());

    // the page's head: title, creator, dates
    let head: Vec<&Line> = all[..first_heading].iter().filter(|(p, _)| *p == 0).map(|(_, l)| *l).collect();
    let title_size = head.iter().map(|l| l.size()).fold(0.0, f64::max);
    let last_title = head.iter().rposition(|l| (l.size() - title_size).abs() < 0.5);
    d.title = doc.outline.first().filter(|t| !t.trim().is_empty()).cloned()
        .unwrap_or_else(|| head.iter().filter(|l| (l.size() - title_size).abs() < 0.5).map(|l| l.text()).collect::<Vec<_>>().join(" "));
    let date = r"(\d{1,2}\.\s*\d{1,2}\.\s*\d{4}|\d{4}-\d{1,2}-\d{1,2}|[A-Za-z]{3,9}\.?\s+\d{1,2},?\s+\d{4}|\d{1,2}\s+[A-Za-z]{3,9}\.?\s+\d{4})";
    let head_text = head.iter().map(|l| l.text()).collect::<Vec<_>>().join("\n");
    for (key, slot) in [("updated", &mut d.updated), ("published", &mut d.published)] {
        let re = regex::Regex::new(&format!(r"(?i)\b{key}\s*:?\s*{date}")).unwrap();
        *slot = re.captures(&head_text).and_then(|c| iso_date(&c[1]));
    }
    if let Some(t) = last_title {
        d.author = head[t + 1..].iter().map(|l| l.text()).find(|s| {
            let letters = s.chars().any(char::is_alphabetic);
            letters && s.to_uppercase() != *s && !s.to_lowercase().starts_with("updated") && !s.to_lowercase().starts_with("published")
        });
    }
    if d.author.is_none() {
        d.author = d.author_url.as_ref().map(|u| u.rsplit('@').next().unwrap_or("").to_string()).filter(|a| !a.is_empty());
    }

    // the sections, by heading
    let mut sections: Vec<(String, Vec<(usize, &Line)>)> = vec![];
    for (p, l) in &all[first_heading..] {
        if is_heading(l) {
            sections.push((l.text(), vec![]));
        } else if let Some(s) = sections.last_mut() {
            s.1.push((*p, l));
        }
    }
    let section = |name: &str| sections.iter().find(|(h, _)| h.to_lowercase().starts_with(name)).map(|(_, ls)| ls.as_slice());

    if let Some(ls) = section("summary").or_else(|| sections.first().map(|(_, ls)| ls.as_slice())) {
        let tags_at = ls.iter().position(|(_, l)| l.text().to_lowercase().starts_with("tags:"));
        let before = &ls[..tags_at.unwrap_or(0)];
        let cat = before.iter().rposition(|(_, l)| l.text().contains(" > "));
        if let Some(c) = cat {
            d.category = Some(ls[c].1.text());
        }
        d.summary = paragraphs(&before[..cat.unwrap_or(before.len())]).split("\n\n").next().unwrap_or("").to_string();
        let mut rest = tags_at.map(|t| t + 1).unwrap_or(0);
        if let Some(t) = tags_at {
            let white: Vec<&Run> = ls[t].1.runs.iter().filter(|r| r.white).collect();
            if white.is_empty() {
                // no chips to tell them by: the words after "Tags:"
                d.tags = ls[t].1.text()[5..].split([',', ' ']).map(str::trim).filter(|w| !w.is_empty()).map(String::from).collect();
            } else {
                d.tags = white.iter().map(|r| r.text.trim().to_string()).collect();
                while let Some((_, l)) = ls.get(rest).filter(|(_, l)| l.runs.iter().all(|r| r.white)) {
                    d.tags.extend(l.runs.iter().map(|r| r.text.trim().to_string()));
                    rest += 1;
                }
            }
            d.tags.retain(|t| !t.is_empty());
        }
        d.description = paragraphs(&ls[rest..]);
        if d.summary.is_empty() {
            d.summary = d.description.split("\n\n").next().unwrap_or("").to_string();
        }
    }
    if let Some(ls) = section("license") {
        let words: Vec<String> = ls.iter().map(|(_, l)| l.text()).take_while(|t| !t.starts_with(['✖', '✔', '✓', '✗', '✘'])).collect();
        d.license_text = words.join(" ");
    }
    d.license = doc.links.iter().find_map(|l| spdx_from_link(l)).or_else(|| spdx_from_text(&d.license_text));
    if let Some(ls) = section("print settings") {
        for (_, l) in ls {
            let t = l.text();
            match t.split_once(':') {
                Some((k, v)) if k.len() < 40 => d.print_settings.push((k.trim().to_string(), v.trim().to_string())),
                _ => {
                    if let Some(last) = d.print_settings.last_mut() {
                        last.1 = format!("{} {t}", last.1).trim().to_string();
                    }
                }
            }
        }
    }
    Some(d)
}

/// Lines into paragraphs (a blank line between them); list items ("•", "◦", "▪") as "- " lines,
/// indented by level.
fn paragraphs(ls: &[(usize, &Line)]) -> String {
    let mut out = String::new();
    let mut prev: Option<(usize, &Line)> = None;
    let mut bullet_x: Vec<f64> = vec![];
    for &(p, l) in ls {
        let mut t = l.text();
        let bullet = t.starts_with(['•', '◦', '▪', '‣', '-', '–']) && t.chars().nth(1) == Some(' ');
        let joined = match prev {
            None => false,
            Some((pp, pl)) if pp == p => l.y() - pl.y() < l.size() * 1.6,
            // over a page: the paragraph goes on unless the last line ended a sentence
            Some((_, pl)) => !pl.text().ends_with(['.', '!', '?', ':']),
        };
        if bullet {
            let x = l.x();
            bullet_x.retain(|b| *b <= x + 1.0);
            if bullet_x.last().is_none_or(|b| (b - x).abs() > 1.0) {
                bullet_x.push(x);
            }
            t = format!("{}- {}", "  ".repeat(bullet_x.len() - 1), t.chars().skip(2).collect::<String>().trim());
            if !out.is_empty() {
                out.push_str(if joined { "\n" } else { "\n\n" });
            }
        } else if joined {
            out.push(' ');
        } else {
            bullet_x.clear();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
        }
        out.push_str(&t);
        prev = Some((p, l));
    }
    out
}

/// "20. 10. 2025", "2025-10-20", "Oct 20, 2025" or "20 Oct 2025" as "2025-10-20".
fn iso_date(s: &str) -> Option<String> {
    let n: Vec<u32> = s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
    let month = |s: &str| {
        let l = s.to_lowercase();
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| l.contains(m)).map(|i| i as u32 + 1)
    };
    let (y, m, d) = if s.contains('-') && n.len() == 3 {
        (n[0], n[1], n[2])
    } else if s.chars().any(char::is_alphabetic) && n.len() == 2 {
        (n[1], month(s)?, n[0])
    } else if n.len() == 3 {
        (n[2], n[1], n[0])
    } else {
        return None;
    };
    ((1..=12).contains(&m) && (1..=31).contains(&d) && y > 1990).then(|| format!("{y:04}-{m:02}-{d:02}"))
}

/// A license's SPDX id from its address (creativecommons.org, gnu.org, opensource.org).
pub fn spdx_from_link(l: &str) -> Option<String> {
    let l = l.to_lowercase();
    let cc = regex::Regex::new(r"creativecommons\.org/licenses/([a-z-]+)/(\d\.\d)").unwrap();
    if let Some(c) = cc.captures(&l) {
        return Some(format!("CC-{}-{}", c[1].to_uppercase(), &c[2]));
    }
    if l.contains("creativecommons.org/publicdomain/zero/1.0") {
        return Some("CC0-1.0".into());
    }
    let gnu = regex::Regex::new(r"gnu\.org/licenses/(l?gpl|agpl)-(\d\.\d)").unwrap();
    if let Some(c) = gnu.captures(&l) {
        return Some(format!("{}-{}", c[1].to_uppercase(), &c[2]));
    }
    None
}

/// A license's SPDX id from how Printables words it.
pub fn spdx_from_text(t: &str) -> Option<String> {
    let l = t.to_lowercase();
    let version = regex::Regex::new(r"(\d\.\d)").unwrap().captures(&l).map(|c| c[1].to_string());
    if l.contains("creative commons") {
        if l.contains("public domain") || l.contains("cc0") {
            return Some("CC0-1.0".into());
        }
        if !l.contains("attribution") {
            return None;
        }
        let squash = l.replace(['-', '—', '–', ' '], "");
        let mut id = "CC-BY".to_string();
        if squash.contains("noncommercial") {
            id.push_str("-NC");
        }
        if squash.contains("sharealike") {
            id.push_str("-SA");
        } else if squash.contains("noderivatives") || squash.contains("noderivs") {
            id.push_str("-ND");
        }
        return Some(format!("{id}-{}", version.unwrap_or_else(|| "4.0".into())));
    }
    let v = version.unwrap_or_default();
    if l.contains("lesser general public license") {
        return Some(if v.starts_with('3') { "LGPL-3.0" } else { "LGPL-2.1" }.into());
    }
    if l.contains("general public license") || l.contains("gnu gpl") {
        return Some(if v.starts_with('2') { "GPL-2.0" } else { "GPL-3.0" }.into());
    }
    if regex::Regex::new(r"\bmit\b").unwrap().is_match(&l) {
        return Some("MIT".into());
    }
    if l.contains("standard digital file license") {
        return Some("LicenseRef-Printables-SDFL".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/printables-opengrid.pdf")).unwrap()
    }

    #[test]
    fn reads_a_printables_page() {
        let doc = read(&sample()).unwrap();
        assert_eq!(doc.producer.as_deref(), Some("WeasyPrint 66.0"));
        let d = printables(&doc).unwrap();
        assert_eq!(d.title, "openGrid - Wall/Desk mounting framework and ecosystem");
        assert_eq!(d.url.as_deref(), Some("https://www.printables.com/model/1214361-opengrid-walldesk-mounting-framework-and-ecosystem"));
        assert_eq!(d.author.as_deref(), Some("David D"));
        assert_eq!(d.author_url.as_deref(), Some("https://www.printables.com/@DavidD"));
        assert_eq!(d.summary, "This is openGrid, the open wall/desk mounting framework.");
        assert_eq!(d.category.as_deref(), Some("Hobby & Makers > Organizers"));
        assert_eq!(d.tags, ["board", "wallmount", "framework", "remixfriendly", "ecosystem", "multiconnect", "opengrid"]);
        assert_eq!((d.published.as_deref(), d.updated.as_deref()), (Some("2025-10-20"), Some("2025-10-20")));
        assert_eq!(d.license.as_deref(), Some("CC-BY-4.0"));
        assert_eq!(d.license_text, "This work is licensed under a Creative Commons (4.0 International License) Attribution");
        assert!(d.description.starts_with("This is openGrid, a general use wall and desk mounting framework/ ecosystem. openGrid uses"), "{}", d.description);
        assert!(d.description.contains("\n\nVersion history:\n\n- 2025-03-05: Initial release\n- 2025-08-01:\n  - Fixed issue"), "{}", d.description);
        assert!(d.description.contains("\n    - This version includes additional holders to attach more firmly to the board\n  - Added v2"), "{}", d.description);
        assert!(d.print_settings.is_empty());
    }

    /// A small PDF as a simpler program might write it: a standard font (no ToUnicode map), text
    /// placed from the bottom of the page, nothing compressed, no bookmarks.
    fn handmade() -> Vec<u8> {
        let content = "BT /F1 20 Tf 300 780 Td (Tidy Tray) Tj ET\n\
            BT /F1 12 Tf 300 755 Td (Sam Maker) Tj ET\n\
            BT /F1 9 Tf 300 740 Td (updated Oct 2, 2025 | published 30 Sep 2025) Tj ET\n\
            BT /F1 16 Tf 50 700 Td (Summary) Tj ET\n\
            BT /F1 11 Tf 50 680 Td (A tray for small parts.) Tj ET\n\
            BT /F1 11 Tf 50 660 Td (Household > Organizers) Tj ET\n\
            BT /F1 11 Tf 50 640 Td (Tags:) Tj 1 1 1 rg 40 0 Td (tray) Tj 40 0 Td (desk tidy) Tj 0 g ET\n\
            BT /F1 11 Tf 50 610 Td (Prints without supports \\(really\\).) Tj ET\n\
            BT /F1 16 Tf 50 570 Td (Print settings) Tj ET\n\
            BT /F1 11 Tf 14 TL 50 550 Td (Printer: Prusa MK4) Tj T* (Infill: 15%) Tj T* (with gyroid) Tj ET\n\
            BT /F1 16 Tf 50 500 Td (License) Tj ET\n\
            BT /F1 11 Tf 50 480 Td [(Creative Commons )-300(\\(4.0 International License\\))] TJ 0 -15 Td (Attribution-Noncommercial-Share Alike) Tj ET\n";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Annots [6 0 R] >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /A << /S /URI /URI (https://www.printables.com/model/42-tidy-tray?lang=en) >> >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        for (i, o) in objs.iter().enumerate() {
            pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        pdf.extend(b"trailer << /Root 1 0 R >>\n%%EOF\n");
        pdf
    }

    #[test]
    fn reads_a_simpler_pdf() {
        let doc = read(&handmade()).unwrap();
        let d = printables(&doc).unwrap();
        assert_eq!(d.title, "Tidy Tray");
        assert_eq!(d.url.as_deref(), Some("https://www.printables.com/model/42-tidy-tray"));
        assert_eq!(d.author.as_deref(), Some("Sam Maker"));
        assert_eq!((d.published.as_deref(), d.updated.as_deref()), (Some("2025-09-30"), Some("2025-10-02")));
        assert_eq!(d.summary, "A tray for small parts.");
        assert_eq!(d.category.as_deref(), Some("Household > Organizers"));
        assert_eq!(d.tags, ["tray", "desk tidy"]);
        assert_eq!(d.description, "Prints without supports (really).");
        assert_eq!(d.print_settings, [("Printer".into(), "Prusa MK4".into()), ("Infill".into(), "15% with gyroid".into())]);
        assert_eq!(d.license.as_deref(), Some("CC-BY-NC-SA-4.0"));
    }

    #[test]
    fn other_pdfs_and_damage() {
        assert!(read(b"not a pdf at all").is_err());
        // a PDF that isn't a Printables page
        let plain = String::from_utf8(handmade()).unwrap().replace("printables.com", "example.com");
        assert_eq!(printables(&read(plain.as_bytes()).unwrap()), None);
        // cut off half way: what's there is still read
        let half = handmade();
        let doc = read(&half[..half.len() * 2 / 3]);
        assert!(doc.is_ok_and(|d| d.pages.len() <= 1));
    }

    #[test]
    fn licenses_and_dates() {
        assert_eq!(spdx_from_text("Creative Commons (4.0 International License) Attribution—Noncommercial—No Derivatives").as_deref(), Some("CC-BY-NC-ND-4.0"));
        assert_eq!(spdx_from_text("Creative Commons Public Domain").as_deref(), Some("CC0-1.0"));
        assert_eq!(spdx_from_text("GNU General Public License v2.0").as_deref(), Some("GPL-2.0"));
        assert_eq!(spdx_from_text("GNU Lesser General Public License v3.0").as_deref(), Some("LGPL-3.0"));
        assert_eq!(spdx_from_text("Standard Digital File License").as_deref(), Some("LicenseRef-Printables-SDFL"));
        assert_eq!(spdx_from_text("All rights reserved"), None);
        assert_eq!(spdx_from_link("https://creativecommons.org/licenses/by-sa/3.0/").as_deref(), Some("CC-BY-SA-3.0"));
        assert_eq!(iso_date("20. 10. 2025").as_deref(), Some("2025-10-20"));
        assert_eq!(iso_date("2025-1-5").as_deref(), Some("2025-01-05"));
        assert_eq!(iso_date("Oct 2, 2025").as_deref(), Some("2025-10-02"));
        assert_eq!(iso_date("13. 13. 2025"), None);
    }
}
