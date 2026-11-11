//! The rules, applied: one constructor per kind of value, each combining the
//! sizes of what it holds as the format's `sizes` say ([`comline_catalog::Sizes`],
//! defined in the specification's `catalog/readme.md`).
//!
//! A [`Size`] is a range, and with a witness budget also two values written
//! in the specification's typed notation (`{"u8": 0}`, `{"struct": ...}`):
//! the least one and the most one, which encode to exactly `min` and `max`.
//! Both come from the same code, so what a witness is can't drift from what
//! the range says.

use std::borrow::Cow;

use comline_catalog::{Body, EnumTag, Header, Primitive, Sizes};
use serde_json::{json, Number, Value};

use super::Range;

/// The most items a count, or bytes a length, any official format writes
/// (`catalog/readme.md`: "counts and lengths are at most 2^32 - 1"). A bound
/// above it is cut to it, and a *fixed* count or length above it can't be
/// written at all.
pub const MAX_COUNT: u64 = u32::MAX as u64;

/// A minimum that means "no finite value": a type that has to contain
/// itself, or whose bytes overflow a `u64`. Saturating arithmetic keeps it.
pub(super) const NO_VALUE: u64 = u64::MAX;

/// Whether the header says a count or a length, which is then at most
/// [`MAX_COUNT`]. `none` says nothing; a delimited one is by brackets.
fn counted(header: &Header) -> bool {
    matches!(header, Header::VarintLeb128 | Header::MsgpackStr | Header::MsgpackArray)
}

#[derive(Debug, Clone)]
struct Witness {
    value: Value,
    /// What building it took: a node each, and a character each.
    cost: usize,
}

/// What a type takes: the range, and the witnesses where they were asked for.
#[derive(Debug, Clone)]
pub(super) struct Size {
    pub range: Range,
    min_witness: Option<Witness>,
    max_witness: Option<Witness>,
}

impl Size {
    pub fn min_witness(&self) -> Option<&Value> {
        self.min_witness.as_ref().map(|w| &w.value)
    }

    pub fn max_witness(&self) -> Option<&Value> {
        self.max_witness.as_ref().map(|w| &w.value)
    }

    fn bare(min: u64, max: Option<u64>, upper_bound: bool) -> Self {
        Size { range: Range { min, max, upper_bound }, min_witness: None, max_witness: None }
    }
}

pub(super) struct Sizer<'r> {
    rules: &'r Sizes,
    /// How much a witness may take to build (a node or a character each);
    /// `None` builds none.
    budget: Option<usize>,
}

fn add(a: u64, b: u64) -> u64 {
    a.saturating_add(b)
}

fn add_max(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    a?.checked_add(b?)
}

impl<'r> Sizer<'r> {
    pub fn new(rules: &'r Sizes, budget: Option<usize>) -> Self {
        Sizer { rules, budget }
    }

    fn wants_witnesses(&self) -> bool {
        self.budget.is_some()
    }

    /// A witness, kept only if it is within the budget.
    fn keep(&self, value: Value, cost: usize) -> Option<Witness> {
        (cost <= self.budget?).then_some(Witness { value, cost })
    }

    /// A type that has to contain itself, with no `@max_depth` to end it:
    /// no finite value, and no end to a value that may.
    pub fn cyclic(&self) -> Size {
        Size::bare(NO_VALUE, None, false)
    }

    pub fn scalar(&self, primitive: Primitive) -> Size {
        let width = self.rules.scalars.of(primitive);
        let mut size = Size::bare(width.min, Some(width.max), width.upper_bound);
        if self.wants_witnesses() {
            let (min, max) = scalar_witnesses(primitive, self.rules.string.body);
            size.min_witness = self.keep(min, 1);
            // A bound no value reaches has no witness.
            size.max_witness = if width.upper_bound { None } else { self.keep(max, 1) };
        }
        size
    }

    /// `char[max N]`, or `string` (`max_chars` `None`).
    pub fn string(&self, max_chars: Option<u64>) -> Option<Size> {
        self.string_of(0, max_chars)
    }

    /// `char[N]`: exactly N characters.
    pub fn fixed_string(&self, chars: u64) -> Option<Size> {
        self.string_of(chars, Some(chars))
    }

    fn string_of(&self, min_chars: u64, max_chars: Option<u64>) -> Option<Size> {
        let rule = &self.rules.string;
        let per = rule.body.max_per_char();
        let counted = counted(&rule.header);
        // A character is at least a byte, so a length that can't be said
        // can't be written.
        if counted && min_chars > MAX_COUNT {
            return None;
        }
        let min = add(rule.header.len(min_chars), min_chars);
        let max_chars = max_chars.map(|n| if counted { n.min(MAX_COUNT / per) } else { n });
        let max = max_chars.and_then(|n| {
            let bytes = n.checked_mul(per)?;
            rule.header.len(bytes).checked_add(bytes)
        });
        let mut size = Size::bare(min, max, false);
        if self.wants_witnesses() {
            let (least, most) = match rule.body {
                Body::Utf8 => ('a', '\u{1F600}'),
                Body::JsonEscaped => ('a', '\u{1}'),
            };
            size.min_witness = self.string_witness(least, min_chars);
            size.max_witness = max_chars.filter(|_| max.is_some()).and_then(|n| self.string_witness(most, n));
        }
        Some(size)
    }

    fn string_witness(&self, c: char, chars: u64) -> Option<Witness> {
        let chars = usize::try_from(chars).ok()?;
        if chars > self.budget? {
            return None;
        }
        self.keep(json!({ "string": c.to_string().repeat(chars) }), chars.max(1))
    }

    /// `T[]` (`max` `None`) or `T[max N]`. `item` is `None` where no value of
    /// it exists (a recursive type past its `@max_depth`): the array is then
    /// empty.
    pub fn array(&self, item: Option<&Size>, max: Option<u64>) -> Option<Size> {
        self.array_of(&self.rules.array, item, 0, max, false)
    }

    /// `T[N]`.
    pub fn fixed_array(&self, item: Option<&Size>, len: u64) -> Option<Size> {
        self.array_of(&self.rules.fixed_array, item, len, Some(len), true)
    }

    fn array_of(
        &self,
        rule: &comline_catalog::SeqSize,
        item: Option<&Size>,
        min_count: u64,
        max_count: Option<u64>,
        fixed: bool,
    ) -> Option<Size> {
        let header = rule.header;
        let counted = counted(&header);
        if counted && min_count > MAX_COUNT {
            return None;
        }
        let max_count = max_count.map(|c| if counted { c.min(MAX_COUNT) } else { c });
        let between = header.between();
        let key = if fixed { "tuple" } else { "array" };
        let empty = || {
            let bytes = header.len(0);
            let mut size = Size::bare(bytes, Some(bytes), false);
            if self.wants_witnesses() {
                size.min_witness = self.keep(json!({ key: [] }), 1);
                size.max_witness = size.min_witness.clone();
            }
            size
        };
        let Some(item) = item else {
            // Nothing to put in it: only an empty array can be written.
            return (min_count == 0).then(empty);
        };

        let min = add(
            add(header.len(min_count), min_count.saturating_mul(item.range.min)),
            between.saturating_mul(min_count.saturating_sub(1)),
        );
        let max = match max_count {
            Some(0) => Some(header.len(0)),
            Some(count) => {
                let items = item.range.max.and_then(|m| count.checked_mul(m));
                let separators = between.checked_mul(count - 1);
                add_max(add_max(Some(header.len(count)), items), separators)
            }
            None => None,
        };
        let mut size = Size::bare(min, max, item.range.upper_bound && max_count != Some(0));
        if self.wants_witnesses() {
            size.min_witness = repeat(self, key, item.min_witness.as_ref(), min_count);
            size.max_witness = match (max_count, size.range.upper_bound) {
                (Some(count), false) if max.is_some() => repeat(self, key, item.max_witness.as_ref(), count),
                _ => None,
            };
        }
        Some(size)
    }

    /// An optional field's type: `none` bytes when absent, `some` and the
    /// value when present. `item` is `None` where no value of it exists: it
    /// is then absent.
    pub fn option(&self, item: Option<&Size>) -> Size {
        let rule = &self.rules.option;
        let absent = |size: &mut Size| {
            if self.wants_witnesses() {
                size.min_witness = self.keep(json!({ "option": null }), 1);
                size.max_witness = size.min_witness.clone();
            }
        };
        let Some(item) = item else {
            let mut size = Size::bare(rule.none, Some(rule.none), false);
            absent(&mut size);
            return size;
        };
        let some_min = add(rule.some, item.range.min);
        let some_max = item.range.max.and_then(|m| m.checked_add(rule.some));
        let min = rule.none.min(some_min);
        // An absent value is as long as `none`; a longer one is the value.
        let max = some_max.map(|m| m.max(rule.none));
        let mut size = Size::bare(min, max, item.range.upper_bound && some_max.is_some_and(|m| m > rule.none));
        if self.wants_witnesses() {
            let present = |w: Option<&Witness>| {
                let w = w?;
                self.keep(json!({ "option": w.value }), w.cost + 1)
            };
            size.min_witness = if rule.none <= some_min {
                self.keep(json!({ "option": null }), 1)
            } else {
                present(item.min_witness.as_ref())
            };
            size.max_witness = match (some_max, size.range.upper_bound) {
                (_, true) | (None, _) => None,
                (Some(m), false) if rule.none >= m => self.keep(json!({ "option": null }), 1),
                (Some(_), false) => present(item.max_witness.as_ref()),
            };
        }
        size
    }

    /// A struct: its fields in order, each `None` where no value of it
    /// exists (then neither does one of the struct).
    pub fn structure(&self, name: &str, fields: &[(&str, Option<Size>)]) -> Option<Size> {
        let rule = &self.rules.struct_;
        let mut items = Vec::with_capacity(fields.len());
        for (field, size) in fields {
            items.push((*field, size.as_ref()?));
        }
        let count = items.len() as u64;
        let key_extra = rule.key_extra;
        let fixed = add(
            rule.header.len(count),
            rule.header.between().saturating_mul(count.saturating_sub(1)),
        );
        let keys: u64 = match key_extra {
            Some(extra) => items.iter().map(|(n, _)| n.len() as u64 + extra).sum(),
            None => 0,
        };
        let min = items.iter().fold(add(fixed, keys), |sum, (_, s)| add(sum, s.range.min));
        let max = items.iter().fold(fixed.checked_add(keys), |sum, (_, s)| add_max(sum, s.range.max));
        let upper_bound = items.iter().any(|(_, s)| s.range.upper_bound);
        let mut size = Size::bare(min, max, upper_bound);
        if self.wants_witnesses() {
            let build = |pick: fn(&Size) -> Option<&Witness>| {
                let mut cost = 1;
                let mut values = Vec::with_capacity(items.len());
                for (field, s) in &items {
                    let w = pick(s)?;
                    cost += w.cost;
                    values.push(json!([field, w.value]));
                }
                self.keep(json!({ "struct": { "name": name, "fields": values } }), cost)
            };
            size.min_witness = build(|s| s.min_witness.as_ref());
            size.max_witness = if max.is_some() && !upper_bound { build(|s| s.max_witness.as_ref()) } else { None };
        }
        Some(size)
    }

    /// An enum of unit variants. With none, no value exists.
    pub fn enumeration(&self, name: &str, variants: &[Cow<'_, str>]) -> Option<Size> {
        let rule = &self.rules.enum_;
        // Each variant's bytes, in order.
        let widths: Vec<u64> = match rule.tag {
            EnumTag::VarintIndex => (0..variants.len() as u64).map(|i| Header::VarintLeb128.len(i)).collect(),
            EnumTag::Name(header) => variants
                .iter()
                .map(|v| add(header.len(v.len() as u64), v.len() as u64))
                .collect(),
        };
        let min = *widths.iter().min()?;
        let max = *widths.iter().max()?;
        let mut size = Size::bare(min, Some(max), false);
        if self.wants_witnesses() {
            let variant = |at: usize| {
                let value = json!({ "unit_variant": { "enum": name, "index": at, "variant": variants[at] } });
                self.keep(value, 1)
            };
            size.min_witness = variant(widths.iter().position(|w| *w == min)?);
            size.max_witness = variant(widths.iter().position(|w| *w == max)?);
        }
        Some(size)
    }
}

/// `count` of `item` as an array (`key` `array`) or a tuple.
fn repeat(sizer: &Sizer, key: &str, item: Option<&Witness>, count: u64) -> Option<Witness> {
    let item = item?;
    let count = usize::try_from(count).ok()?;
    let cost = item.cost.checked_mul(count)?.checked_add(1)?;
    if cost > sizer.budget? {
        return None;
    }
    let items = vec![item.value.clone(); count];
    sizer.keep(json!({ key: items }), cost)
}

/// The least and the most bytes one value of a primitive takes, as a value:
/// chosen to be both in every official format, which the vectors hold them
/// to (`sizes-<format>.json`). Where a format writes every value of a
/// primitive alike, any is both.
fn scalar_witnesses(primitive: Primitive, body: Body) -> (Value, Value) {
    let float = |f: f64| Value::Number(Number::from_f64(f).expect("a finite float"));
    match primitive {
        Primitive::Bool => (json!({ "bool": true }), json!({ "bool": false })),
        Primitive::U8 => (json!({ "u8": 0 }), json!({ "u8": 255 })),
        Primitive::U16 => (json!({ "u16": 0 }), json!({ "u16": 65535 })),
        Primitive::U32 => (json!({ "u32": 0 }), json!({ "u32": 4294967295u32 })),
        Primitive::U64 => (json!({ "u64": "0" }), json!({ "u64": u64::MAX.to_string() })),
        Primitive::U128 => (json!({ "u128": "0" }), json!({ "u128": u128::MAX.to_string() })),
        Primitive::S8 => (json!({ "i8": 0 }), json!({ "i8": -128 })),
        Primitive::S16 => (json!({ "i16": 0 }), json!({ "i16": -32768 })),
        Primitive::S32 => (json!({ "i32": 0 }), json!({ "i32": i32::MIN })),
        Primitive::S64 => (json!({ "i64": "0" }), json!({ "i64": i64::MIN.to_string() })),
        Primitive::S128 => (json!({ "i128": "0" }), json!({ "i128": i128::MIN.to_string() })),
        // The longest text `serde_json` writes: a number the shortest form
        // of which is all of its digits, in positional notation.
        Primitive::F32 => (json!({ "f32": float(0.0) }), json!({ "f32": float(-6233676316672.0) })),
        Primitive::F64 => (json!({ "f64": float(0.0) }), json!({ "f64": float(-1.2610470545525327e-05) })),
        Primitive::Char => {
            let most = match body {
                Body::Utf8 => '\u{1F600}',
                Body::JsonEscaped => '\u{1}',
            };
            (json!({ "char": "a" }), json!({ "char": most.to_string() }))
        }
        Primitive::Unit => (json!({ "unit": null }), json!({ "unit": null })),
    }
}
