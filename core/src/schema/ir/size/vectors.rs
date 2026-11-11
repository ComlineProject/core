//! The specification's size vectors, held against the sizer: for every case
//! of `vectors/sizes-<format>.json`, the range computed from the format's
//! rules for the case's type is the case's `[min, max]`.
//!
//! The vectors are in `tests/spec/vectors`, vendored at the rev
//! `comline-catalog` is pinned to (`tests/spec/vendor.sh`; a test here fails
//! when the two disagree). A git dependency's checkout isn't reachable from a
//! test without asking cargo, so the vectors are held beside the pin instead
//! of read from it; the check keeps them the pin's own.
//!
//! A case's type (`#SizeType`) is richer than the IR's: it has an option
//! anywhere, where the IR has one only on a field. Each type that is
//! expressible converts to the [`TypeRef`] the compiler freezes, with its
//! declarations in a lookup, and goes through the same walk as any; one with
//! an option in an array is built from the sizer's own constructors.
//!
//! Core can't encode, so the witnesses aren't encoded here: the runtime
//! encodes the cases' own (`vectors/`), and the differential fuzz in the
//! runtime encodes core's. What is checked here: that core builds a witness
//! of the type (the shape and the bounds), and that for each primitive the
//! ones it builds are values the case itself measures as the least and the
//! most.

use std::path::PathBuf;

use serde_json::Value;

use super::sizer::Size;
use super::*;
use crate::schema::ir::frozen::types::{PrimitiveType, TypeRef};

/// What a witness may take to build: more than any case needs.
const BUDGET: usize = 1 << 20;

fn spec_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/spec")
}

struct Case {
    id: String,
    format: String,
    ty: Value,
    range: Range,
    /// Every witness the case states, `min`, `max` and `between`: a value and its bytes.
    witnesses: Vec<(Value, usize)>,
    has_max_witness: bool,
}

fn cases(file: &str) -> Vec<Case> {
    let path = spec_dir().join("vectors").join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let vectors: Value = serde_json::from_str(&text).unwrap();
    vectors["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            assert_eq!(case["op"], "size", "{}", case["id"]);
            let input = &case["input"];
            let witnesses = &input["witnesses"];
            let measured = |w: &Value| {
                let bytes = w["bytes"].as_str().unwrap().len() / 2;
                (w["value"].clone(), bytes)
            };
            let mut all = vec![measured(&witnesses["min"])];
            all.extend(witnesses.get("max").map(measured));
            all.extend(witnesses.get("between").and_then(Value::as_array).into_iter().flatten().map(measured));
            Case {
                id: case["id"].as_str().unwrap().to_string(),
                format: input["format"].as_str().unwrap().to_string(),
                ty: input["type"].clone(),
                range: Range {
                    min: input["min"].as_u64().unwrap(),
                    max: input["max"].as_u64(),
                    upper_bound: input.get("upper_bound").is_some(),
                },
                witnesses: all,
                has_max_witness: witnesses.get("max").is_some(),
            }
        })
        .collect()
}

/// A declaration of a case's type.
enum Declared {
    Struct { name: String, fields: Vec<(String, TypeRef, bool)>, max_depth: Option<u64> },
    Enum { name: String, variants: Vec<String> },
}

#[derive(Default)]
struct Types {
    declared: Vec<Declared>,
}

impl Lookup for Types {
    type Scope = ();

    fn resolve(&self, _: (), name: &str) -> Option<Resolved<'_, ()>> {
        let at = self.declared.iter().position(|d| match d {
            Declared::Struct { name: n, .. } | Declared::Enum { name: n, .. } => n == name,
        })?;
        let decl = match &self.declared[at] {
            Declared::Struct { name, fields, max_depth } => Decl::Struct {
                name: name.as_str().into(),
                fields: fields
                    .iter()
                    .map(|(n, ty, optional)| Member { name: n.as_str().into(), ty: std::borrow::Cow::Borrowed(ty), optional: *optional })
                    .collect(),
                max_depth: *max_depth,
            },
            Declared::Enum { name, variants } => Decl::Enum {
                name: name.as_str().into(),
                variants: variants.iter().map(|v| v.as_str().into()).collect(),
            },
        };
        Some(Resolved { key: at + 1, scope: (), decl })
    }
}

/// A case's type: the IR's where it can be, the rest by its constructors.
enum Conv {
    Ref(TypeRef),
    Option(Box<Conv>),
    Array(Box<Conv>, Option<u64>),
    Fixed(Box<Conv>, u64),
}

impl Conv {
    fn array(self, max: Option<u64>) -> Conv {
        match self {
            Conv::Ref(element) => {
                Conv::Ref(TypeRef::Array { element: Box::new(element), length: None, max })
            }
            other => Conv::Array(Box::new(other), max),
        }
    }

    fn fixed(self, len: u64) -> Conv {
        match self {
            Conv::Ref(element) => {
                Conv::Ref(TypeRef::Array { element: Box::new(element), length: Some(len), max: None })
            }
            other => Conv::Fixed(Box::new(other), len),
        }
    }

    fn eval(&self, walk: &mut Walk<'_, Types>) -> Out {
        match self {
            Conv::Ref(ty) => walk.size(ty, ()),
            Conv::Option(item) => {
                let item = item.eval(walk)?;
                Ok(Some(walk.sizer.option(item.as_ref())))
            }
            Conv::Array(item, max) => {
                let item = item.eval(walk)?;
                Ok(walk.sizer.array(item.as_ref(), *max))
            }
            Conv::Fixed(item, len) => {
                let item = item.eval(walk)?;
                Ok(walk.sizer.fixed_array(item.as_ref(), *len))
            }
        }
    }
}

fn scalar(name: &str) -> Option<TypeRef> {
    use PrimitiveType as P;
    Some(match name {
        "bool" => TypeRef::Primitive(P::Bool),
        "u8" => TypeRef::Primitive(P::U8),
        "u16" => TypeRef::Primitive(P::U16),
        "u32" => TypeRef::Primitive(P::U32),
        "u64" => TypeRef::Primitive(P::U64),
        "u128" => TypeRef::Primitive(P::U128),
        "i8" => TypeRef::Primitive(P::S8),
        "i16" => TypeRef::Primitive(P::S16),
        "i32" => TypeRef::Primitive(P::S32),
        "i64" => TypeRef::Primitive(P::S64),
        "i128" => TypeRef::Primitive(P::S128),
        "f32" => TypeRef::Primitive(P::F32),
        "f64" => TypeRef::Primitive(P::F64),
        "string" => TypeRef::Primitive(P::String),
        "char" => TypeRef::Primitive(P::Char),
        "unit" => TypeRef::Unit,
        _ => return None,
    })
}

fn the(value: &Value) -> u64 {
    value.as_u64().unwrap()
}

/// `ty` as a [`Conv`]. `this` is the struct `self` means, where one does.
fn convert(ty: &Value, types: &mut Types, this: Option<&str>) -> Conv {
    if let Some(name) = ty.as_str() {
        return Conv::Ref(match name {
            "self" => TypeRef::Named(this.expect("`self` in a recursive type").to_string()),
            name => scalar(name).unwrap_or_else(|| panic!("a type {name}")),
        });
    }
    let (kind, body) = ty.as_object().unwrap().iter().next().unwrap();
    let chars = |length, max| {
        Conv::Ref(TypeRef::Array {
            element: Box::new(TypeRef::Primitive(PrimitiveType::Char)),
            length,
            max,
        })
    };
    match kind.as_str() {
        "bounded_string" => chars(None, Some(the(&body["max"]))),
        "fixed_string" => chars(Some(the(&body["len"])), None),
        "option" => Conv::Option(Box::new(convert(body, types, this))),
        "array" => convert(body, types, this).array(None),
        "fixed" => convert(&body["of"], types, this).fixed(the(&body["len"])),
        "bounded_array" => convert(&body["of"], types, this).array(Some(the(&body["max"]))),
        "recursive" => declare(&body["of"], types, Some(the(&body["max_depth"]))),
        "struct" => declare(ty, types, None),
        "enum" => {
            let name = body["name"].as_str().unwrap().to_string();
            let variants = body["variants"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string());
            types.declared.push(Declared::Enum { name: name.clone(), variants: variants.collect() });
            Conv::Ref(TypeRef::Named(name))
        }
        other => panic!("a type {other}"),
    }
}

/// The struct `ty` is, declared, with its `@max_depth` where it is the
/// recursive one.
fn declare(ty: &Value, types: &mut Types, max_depth: Option<u64>) -> Conv {
    let body = &ty["struct"];
    let name = body["name"].as_str().unwrap().to_string();
    let mut fields = vec![];
    for field in body["fields"].as_array().unwrap() {
        let field_name = field[0].as_str().unwrap().to_string();
        // An optional field is `option` in a case's type.
        let (ty, optional) = match field[1].get("option") {
            Some(inner) => (inner, true),
            None => (&field[1], false),
        };
        let Conv::Ref(ty) = convert(ty, types, Some(&name)) else {
            panic!("{name}.{field_name} is a type the IR has no spelling for")
        };
        fields.push((field_name, ty, optional));
    }
    types.declared.push(Declared::Struct { name: name.clone(), fields, max_depth });
    Conv::Ref(TypeRef::Named(name))
}

/// Whether `value` is of the case's `ty`: its shape, and its bounds.
fn conforms(value: &Value, ty: &Value, this: Option<(&Value, u64, u64)>) -> bool {
    let of = |key: &str| value.get(key);
    if let Some(name) = ty.as_str() {
        return match name {
            "self" => {
                let (of_ty, max_depth, level) = this.expect("`self` in a recursive type");
                level < max_depth && conforms(value, of_ty, Some((of_ty, max_depth, level + 1)))
            }
            "char" => of("char").and_then(Value::as_str).is_some_and(|s| s.chars().count() == 1),
            "string" => of("string").is_some_and(Value::is_string),
            "unit" => of("unit").is_some_and(Value::is_null),
            // The rest are named as the value says: `bool`, `u8`, `i128`, `f64`...
            number => of(number).is_some(),
        };
    }
    let (kind, body) = ty.as_object().unwrap().iter().next().unwrap();
    let items = |key: &str| of(key).and_then(Value::as_array);
    match kind.as_str() {
        "bounded_string" => of("string").and_then(Value::as_str).is_some_and(|s| s.chars().count() as u64 <= the(&body["max"])),
        "fixed_string" => of("string").and_then(Value::as_str).is_some_and(|s| s.chars().count() as u64 == the(&body["len"])),
        "option" => of("option").is_some_and(|v| v.is_null() || conforms(v, body, this)),
        "array" => items("array").is_some_and(|a| a.iter().all(|v| conforms(v, body, this))),
        "fixed" => items("tuple")
            .is_some_and(|a| a.len() as u64 == the(&body["len"]) && a.iter().all(|v| conforms(v, &body["of"], this))),
        "bounded_array" => items("array")
            .is_some_and(|a| a.len() as u64 <= the(&body["max"]) && a.iter().all(|v| conforms(v, &body["of"], this))),
        "recursive" => conforms(value, &body["of"], Some((&body["of"], the(&body["max_depth"]), 1))),
        "struct" => of("struct").is_some_and(|s| {
            let fields = body["fields"].as_array().unwrap();
            let values = s["fields"].as_array().unwrap();
            s["name"] == body["name"]
                && values.len() == fields.len()
                && fields.iter().zip(values).all(|(f, v)| f[0] == v[0] && conforms(&v[1], &f[1], this))
        }),
        "enum" => of("unit_variant").is_some_and(|v| {
            let at = v["index"].as_u64().unwrap() as usize;
            v["enum"] == body["name"] && body["variants"][at] == v["variant"]
        }),
        other => panic!("a type {other}"),
    }
}

/// The size of a case's type by the rules of its format, and the witnesses.
fn sized(case: &Case) -> Option<Size> {
    let rules = rules(&case.format).unwrap_or_else(|| panic!("{} has no size rules", case.format));
    let mut types = Types::default();
    let conv = convert(&case.ty, &mut types, None);
    run(rules, &types, Some(BUDGET), |walk| conv.eval(walk))
}

fn check(file: &str, count: usize) {
    let cases = cases(file);
    assert_eq!(cases.len(), count, "{file} has {} cases", cases.len());
    for case in &cases {
        let id = &case.id;
        let size = sized(case).unwrap_or_else(|| panic!("{id}: no size"));
        assert_eq!(size.range, case.range, "{id}");

        // Witnesses: a least, and a most where the case has one.
        let min = size.min_witness().unwrap_or_else(|| panic!("{id}: no min witness"));
        assert!(conforms(min, &case.ty, None), "{id}: min witness {min}");
        assert_eq!(size.max_witness().is_some(), case.has_max_witness, "{id}: max witness");
        if let Some(max) = size.max_witness() {
            assert!(conforms(max, &case.ty, None), "{id}: max witness {max}");
        }
        if case.range.min < case.range.max.unwrap_or(u64::MAX) {
            assert_ne!(Some(min), size.max_witness(), "{id}: both witnesses are one value");
        }

        // A primitive: the case measures the very values core builds.
        if case.ty.as_str().is_some_and(|name| scalar(name).is_some() && name != "string") {
            let measured = |value: &Value| {
                case.witnesses.iter().find(|(v, _)| v == value).map(|(_, bytes)| *bytes)
            };
            assert_eq!(measured(min), Some(case.range.min as usize), "{id}: the case doesn't measure {min} as its least");
            if let Some(max) = size.max_witness() {
                assert_eq!(measured(max), case.range.max.map(|m| m as usize), "{id}: the case doesn't measure {max} as its most");
            }
        }
    }
}

#[test]
fn the_vectors_are_the_catalogs_rev() {
    let rev = std::fs::read_to_string(spec_dir().join("REV")).unwrap();
    let manifest = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let pinned = manifest
        .lines()
        .find(|line| line.starts_with("comline-catalog"))
        .and_then(|line| line.split("rev = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("comline-catalog is pinned by rev");
    assert_eq!(
        rev.trim(),
        pinned,
        "tests/spec/vectors are at another rev than `comline-catalog`: run tests/spec/vendor.sh <rev>"
    );
}

#[test]
fn packed_ranges_are_the_specs() {
    check("sizes-packed.json", 59);
}

#[test]
fn msgpack_ranges_are_the_specs() {
    check("sizes-msgpack.json", 60);
}

#[test]
fn json_ranges_are_the_specs() {
    check("sizes-json.json", 57);
}
