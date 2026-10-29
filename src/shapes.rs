//! Stem shapes: how an unknown stem borrows the ending of known ones.
//!
//! A shape is the tail of a known stem allomorph, e.g. `citie` (from `city`
//! before `-s`) gives the tail `tie` with lemma tail `ty`. It keeps
//! everything the compiler attached to that allomorph: continuation, filter,
//! guard and zero morphemes. Hybrid mode reads an unknown stem as a free head
//! plus a shape, so its spelling changes, inflection class and lemma all come
//! from known stems that end the same way. The empty ending is a shape too:
//! it gives a stem the lemma ending its whole class shares (`film` for
//! `filmować`), whatever its last letters. `support` counts those stems and
//! ranks competing guesses. Nothing is learned beyond the lexicon itself.

use std::collections::{BTreeMap, BTreeSet};

use crate::codec::{Reader, Writer};
use crate::error::{art_err, Result};
use crate::morphotactics::{ClassRt, EntryRt};
use crate::spec::{ClassId, Grammar};

/// Known stems a shape needs: one example is an anecdote, not a pattern.
pub const MIN_SUPPORT: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub surface: String,
    pub lemma: String,
    pub class: ClassId,
    /// A known entry with this shape; hybrid mode steps the morphotactics
    /// with it.
    pub entry: u32,
    /// Number of known stem allomorphs with this shape.
    pub support: u32,
}

/// (surface, lemma, class, tag, cont, filter, guard, zeros)
type Key<'a> = (String, String, ClassId, &'a str, ClassId, u16, u32, &'a [String]);

/// Shapes of every open lexicon, sorted by surface (most support first).
/// None in underlying-harmony mode, where a stem's harmony depends on all of
/// it, and none for INDUCE lexicons, whose unknown stems infer exceptions.
/// Stems with irregular forms of their own part of speech listed whole
/// (`ran:run`, tag `V;PST`) are skipped: irregular inflection is
/// unproductive, so new words never follow it. A whole form of another part
/// of speech (`abonując:abonować`, tag `V.CVB`) is just uninflected. Parts of
/// speech compare by the tag's first token, so an irregular `V.PTCP;PST`
/// form does not mark a lemma whose stems are tagged `V`.
pub fn derive(g: &Grammar) -> Vec<Shape> {
    if g.options.underlying {
        return Vec::new();
    }
    let open = |e: &crate::spec::Entry| g.classes.get(e.lexicon as usize).is_some_and(|c| c.open_tag.is_some());
    fn pos(tag: &str) -> &str {
        tag.split([';', '+']).next().unwrap_or_default()
    }
    let irregular: BTreeSet<(&str, &str)> = g.entries.iter().filter(|e| !open(e) && e.surface != e.lemma).map(|e| (e.lemma.as_str(), pos(&e.tag))).collect();
    let mut seen: BTreeMap<Key, (u32, u32)> = BTreeMap::new();
    for (id, e) in g.entries.iter().enumerate() {
        if !open(e) || g.induce_classes.contains(&e.lexicon) || !e.pin_set.is_empty() || !e.rpin_set.is_empty() {
            continue;
        }
        if irregular.contains(&(e.lemma.as_str(), pos(e.tag.as_str()))) {
            continue;
        }
        let s: Vec<char> = e.surface.chars().collect();
        let l: Vec<char> = e.lemma.chars().collect();
        let common = s.iter().zip(&l).take_while(|(a, b)| a == b).count();
        // Every ending, the empty one included: the head before it keeps at
        // least one char.
        for start in (1..=common).rev() {
            let (Some(st), Some(lt)) = (s.get(start..), l.get(start..)) else { break };
            let key = (st.iter().collect(), lt.iter().collect(), e.lexicon, e.tag.as_str(), e.cont, e.filter, e.guard, e.zeros.as_slice());
            seen.entry(key).or_insert((id as u32, 0)).1 += 1;
        }
    }
    let mut out: Vec<Shape> = seen
        .into_iter()
        .filter(|(_, (_, support))| *support >= MIN_SUPPORT)
        .map(|((surface, lemma, class, ..), (entry, support))| Shape { surface, lemma, class, entry, support })
        .collect();
    out.sort_by(|a, b| a.surface.cmp(&b.surface).then(b.support.cmp(&a.support)).then(a.entry.cmp(&b.entry)));
    out
}

/// Indices, within `within`, of the shapes whose surface starts with
/// `prefix` (`shapes` sorted by surface, `within` a prefix range of a
/// shorter prefix or `0..len`). The exact matches come first.
pub fn with_prefix(shapes: &[Shape], within: std::ops::Range<usize>, prefix: &str) -> std::ops::Range<usize> {
    let Some(slice) = shapes.get(within.clone()) else { return 0..0 };
    let lo = slice.partition_point(|s| s.surface.as_str() < prefix);
    let hi = lo + slice.get(lo..).map_or(0, |rest| rest.partition_point(|s| s.surface.starts_with(prefix)));
    within.start + lo..within.start + hi
}

pub fn write(shapes: &[Shape], w: &mut Writer) {
    w.u32(shapes.len() as u32);
    for s in shapes {
        w.str(&s.surface);
        w.str(&s.lemma);
        w.u32(s.class as u32);
        w.u32(s.entry);
        w.u32(s.support);
    }
}

pub fn read(r: &mut Reader, classes: &[ClassRt], entries: &[EntryRt]) -> Result<Vec<Shape>> {
    let n = r.count(20)?;
    let mut out: Vec<Shape> = Vec::with_capacity(n);
    for _ in 0..n {
        let surface = r.string()?;
        let lemma = r.string()?;
        let class = r.u32()?;
        let entry = r.u32()?;
        let support = r.u32()?;
        let open = classes.get(class as usize).is_some_and(|c| c.open_tag.is_some());
        let owner = entries.get(entry as usize).map(|e| e.lexicon as u32);
        if !open || owner != Some(class) {
            return art_err("bad stem shape");
        }
        if out.last().is_some_and(|p| p.surface > surface) {
            return art_err("stem shapes not sorted");
        }
        out.push(Shape { surface, lemma, class: class as ClassId, entry, support });
    }
    Ok(out)
}
