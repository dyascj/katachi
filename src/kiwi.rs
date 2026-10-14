//! A generic decoder for the Kiwi binary format (github.com/evanw/kiwi), driven by a schema
//! that is itself decoded from Kiwi's binary schema encoding.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Byte(u8),
    Int(i64),
    UInt(u64),
    Float(f32),
    Str(String),
    Enum(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }
    pub fn f(&self, key: &str) -> Option<f32> {
        match self.get(key)? {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f32),
            Value::UInt(v) => Some(*v as f32),
            Value::Byte(v) => Some(*v as f32),
            _ => None,
        }
    }
    pub fn u(&self, key: &str) -> Option<u64> {
        match self.get(key)? {
            Value::UInt(v) => Some(*v),
            Value::Int(v) => Some(*v as u64),
            Value::Byte(v) => Some(*v as u64),
            _ => None,
        }
    }
    pub fn s(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(v) | Value::Enum(v) => Some(v),
            _ => None,
        }
    }
    pub fn b(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn arr(&self, key: &str) -> &[Value] {
        match self.get(key) {
            Some(Value::Array(v)) => v,
            _ => &[],
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Value::Array(v) => v.iter().filter_map(|b| if let Value::Byte(x) = b { Some(*x) } else { None }).collect(),
            _ => vec![],
        }
    }
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    /// Negative: builtin (~t indexes BUILTINS); otherwise a definition index.
    ty: i64,
    array: bool,
    value: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum DefKind {
    Enum,
    Struct,
    Message,
}

#[derive(Clone, Debug)]
struct Def {
    name: String,
    kind: DefKind,
    fields: Vec<Field>,
}

pub struct Schema {
    defs: Vec<Def>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.at).ok_or("unexpected end of data")?;
        self.at += 1;
        Ok(b)
    }
    fn var_uint(&mut self) -> Result<u64, String> {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let b = self.byte()?;
            v |= ((b & 127) as u64) << shift;
            shift += 7;
            if b & 128 == 0 || shift >= 35 {
                return Ok(v & 0xFFFF_FFFF);
            }
        }
    }
    fn var_int(&mut self) -> Result<i64, String> {
        let v = self.var_uint()? as u32;
        Ok(if v & 1 != 0 { !(v >> 1) as i32 as i64 } else { (v >> 1) as i64 })
    }
    fn var_uint64(&mut self) -> Result<u64, String> {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let b = self.byte()?;
            v |= ((b & 127) as u64) << shift;
            shift += 7;
            if b & 128 == 0 || shift >= 64 {
                return Ok(v);
            }
        }
    }
    fn var_int64(&mut self) -> Result<i64, String> {
        let v = self.var_uint64()?;
        Ok(if v & 1 != 0 { !(v >> 1) as i64 } else { (v >> 1) as i64 })
    }
    fn float(&mut self) -> Result<f32, String> {
        let first = self.byte()?;
        if first == 0 {
            return Ok(0.0);
        }
        let rest = self.data.get(self.at..self.at + 3).ok_or("unexpected end of data")?;
        self.at += 3;
        let bits = first as u32 | (rest[0] as u32) << 8 | (rest[1] as u32) << 16 | (rest[2] as u32) << 24;
        // NaN and infinity would end up in saved files as nulls; read them as 0.
        Ok(Some(f32::from_bits(bits.rotate_left(23))).filter(|f| f.is_finite()).unwrap_or(0.0))
    }
    fn string(&mut self) -> Result<String, String> {
        let start = self.at;
        while *self.data.get(self.at).ok_or("unterminated string")? != 0 {
            self.at += 1;
        }
        let s = String::from_utf8_lossy(&self.data[start..self.at]).into_owned();
        self.at += 1;
        Ok(s)
    }
}

impl Schema {
    /// Decode a binary Kiwi schema.
    pub fn decode(data: &[u8]) -> Result<Schema, String> {
        let mut r = Reader { data, at: 0 };
        let count = r.var_uint()?;
        let mut defs = vec![];
        for _ in 0..count {
            let name = r.string()?;
            let kind = match r.byte()? {
                0 => DefKind::Enum,
                1 => DefKind::Struct,
                2 => DefKind::Message,
                k => return Err(format!("bad definition kind {k}")),
            };
            let n = r.var_uint()?;
            let mut fields = vec![];
            for _ in 0..n {
                let name = r.string()?;
                let ty = r.var_int()?;
                let array = r.byte()? & 1 != 0;
                let value = r.var_uint()?;
                fields.push(Field { name, ty, array, value });
            }
            defs.push(Def { name, kind, fields });
        }
        Ok(Schema { defs })
    }

    /// Decode a message of the named root type.
    pub fn decode_message(&self, root: &str, data: &[u8]) -> Result<Value, String> {
        let idx = self.defs.iter().position(|d| d.name == root).ok_or(format!("no type {root}"))?;
        let mut r = Reader { data, at: 0 };
        self.read_def(&mut r, idx)
    }

    fn read_def(&self, r: &mut Reader, idx: usize) -> Result<Value, String> {
        let def = &self.defs[idx];
        match def.kind {
            DefKind::Enum => {
                let v = r.var_uint()?;
                Ok(Value::Enum(def.fields.iter().find(|f| f.value == v).map_or_else(|| v.to_string(), |f| f.name.clone())))
            }
            DefKind::Struct => {
                let mut m = BTreeMap::new();
                for f in &def.fields {
                    m.insert(f.name.clone(), self.read_field(r, f)?);
                }
                Ok(Value::Object(m))
            }
            DefKind::Message => {
                let mut m = BTreeMap::new();
                loop {
                    let id = r.var_uint()?;
                    if id == 0 {
                        return Ok(Value::Object(m));
                    }
                    let f = def.fields.iter().find(|f| f.value == id).ok_or_else(|| format!("unknown field {id} in {}", def.name))?;
                    m.insert(f.name.clone(), self.read_field(r, f)?);
                }
            }
        }
    }

    fn read_field(&self, r: &mut Reader, f: &Field) -> Result<Value, String> {
        if f.array {
            let n = r.var_uint()? as usize;
            // Byte arrays (blobs) are common and large; read them directly.
            if f.ty == -2 {
                let bytes = r.data.get(r.at..r.at + n).ok_or("unexpected end of data")?;
                r.at += n;
                return Ok(Value::Array(bytes.iter().map(|b| Value::Byte(*b)).collect()));
            }
            let mut v = Vec::with_capacity(n.min(1 << 16));
            for _ in 0..n {
                v.push(self.read_one(r, f.ty)?);
            }
            Ok(Value::Array(v))
        } else {
            self.read_one(r, f.ty)
        }
    }

    fn read_one(&self, r: &mut Reader, ty: i64) -> Result<Value, String> {
        Ok(match ty {
            -1 => Value::Bool(r.byte()? != 0),
            -2 => Value::Byte(r.byte()?),
            -3 => Value::Int(r.var_int()?),
            -4 => Value::UInt(r.var_uint()?),
            -5 => Value::Float(r.float()?),
            -6 => Value::Str(r.string()?),
            -7 => Value::Int(r.var_int64()?),
            -8 => Value::UInt(r.var_uint64()?),
            t if t >= 0 && (t as usize) < self.defs.len() => self.read_def(r, t as usize)?,
            t => return Err(format!("bad type {t}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal Kiwi writer for tests (schema + message).
    fn vu(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let b = (v & 127) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 128);
        }
    }
    fn vi(out: &mut Vec<u8>, v: i64) {
        vu(out, (((v << 1) ^ (v >> 63)) as u64) & 0xFFFF_FFFF);
    }
    fn st(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }
    fn fl(out: &mut Vec<u8>, f: f32) {
        if f == 0.0 {
            out.push(0);
            return;
        }
        out.extend_from_slice(&f.to_bits().rotate_right(23).to_le_bytes());
    }

    #[test]
    fn decodes_schema_and_message() {
        // enum Kind { A = 0; B = 1; } struct Pt { float x; float y; } message M { string name = 1; Kind kind = 2; Pt[] pts = 3; int n = 4; }
        let mut s = vec![];
        vu(&mut s, 3);
        st(&mut s, "Kind");
        s.push(0);
        vu(&mut s, 2);
        for (n, v) in [("A", 0), ("B", 1)] {
            st(&mut s, n);
            vi(&mut s, 0);
            s.push(0);
            vu(&mut s, v);
        }
        st(&mut s, "Pt");
        s.push(1);
        vu(&mut s, 2);
        for (n, v) in [("x", 1), ("y", 2)] {
            st(&mut s, n);
            vi(&mut s, -5);
            s.push(0);
            vu(&mut s, v);
        }
        st(&mut s, "M");
        s.push(2);
        vu(&mut s, 4);
        for (n, t, arr, v) in [("name", -6, 0u8, 1), ("kind", 0, 0, 2), ("pts", 1, 1, 3), ("n", -3, 0, 4)] {
            st(&mut s, n);
            vi(&mut s, t);
            s.push(arr);
            vu(&mut s, v);
        }
        let schema = Schema::decode(&s).unwrap();
        let mut m = vec![];
        vu(&mut m, 1);
        st(&mut m, "hello");
        vu(&mut m, 2);
        vu(&mut m, 1);
        vu(&mut m, 3);
        vu(&mut m, 2);
        for (x, y) in [(1.5f32, f32::NAN), (0.0, 3.25)] {
            fl(&mut m, x);
            fl(&mut m, y);
        }
        vu(&mut m, 4);
        vi(&mut m, -7);
        vu(&mut m, 0);
        let v = schema.decode_message("M", &m).unwrap();
        assert_eq!(v.s("name"), Some("hello"));
        assert_eq!(v.s("kind"), Some("B"));
        assert_eq!(v.arr("pts")[1].f("y"), Some(3.25));
        assert_eq!(v.arr("pts")[0].f("x"), Some(1.5));
        assert_eq!(v.arr("pts")[0].f("y"), Some(0.0), "NaN reads as 0");
        assert_eq!(v.get("n"), Some(&Value::Int(-7)));
    }
}
