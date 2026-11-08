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
//!
//! A stem can also change at its start or inside: a shape may carry a
//! prefix the lemma lacks or spells differently (`ge|mach` of `machen`,
//! Hebrew `מ|גדיל` of `ה|גדיל`) and one marked letter the lemma writes plain
//! (`hábl` of `hablar`, `Gärt` of `Garten`). The stem and lemma are aligned
//! so the most letters match; a prefix or letter change is used only when it
//! explains more letters than the plain common start does, and every
//! alignment tied for best is kept, so support decides which generalizes. A
//! capitalized lemma of a lowercased stem (`kindheit` of `Kindheit`) is a
//! flag, so such stems share their endings whatever letter they start with.
//! Hebrew final letters match their medial forms, and a lemma ends in its
//! final form.

use std::collections::{BTreeMap, BTreeSet};

use crate::codec::{Reader, Writer};
use crate::error::{art_err, Result};
use crate::morphotactics::{ClassRt, EntryRt};
use crate::normalize::{base_letter, medial, with_final_letter};
use crate::spec::{ClassId, Grammar};

/// Known stems a shape needs: one example is an anecdote, not a pattern.
pub const MIN_SUPPORT: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub surface: String,
    pub lemma: String,
    /// Spelled before the head (`ge`), and what the lemma has there instead.
    pub pre: String,
    pub pre_lemma: String,
    /// A marked letter of the head and the plain letter the lemma has for
    /// it (`á` → `a`); the head's last such letter changes.
    pub sub: Option<(char, char)>,
    /// The lemma capitalizes its first letter (German nouns).
    pub title: bool,
    pub class: ClassId,
    /// A known entry with this shape; hybrid mode steps the morphotactics
    /// with it.
    pub entry: u32,
    /// Number of known stem allomorphs with this shape.
    pub support: u32,
}

impl Shape {
    /// The lemma of an unknown stem whose head (after [`Shape::pre`]) is
    /// `head`; `None` when the head lacks the marked letter.
    pub fn lemma_of(&self, head: &str) -> Option<String> {
        let mut out = self.pre_lemma.clone();
        match self.sub {
            None => out.push_str(head),
            Some((from, to)) => {
                let at = head.rfind(from)?;
                out.push_str(head.get(..at)?);
                out.push(to);
                out.push_str(head.get(at + from.len_utf8()..)?);
            }
        }
        out.push_str(&self.lemma);
        if self.title {
            let mut cs = out.chars();
            out = cs.next().map(|c| c.to_uppercase().chain(cs).collect()).unwrap_or_default();
        }
        Some(with_final_letter(out))
    }

    /// Does the shape say nothing beyond "some stem of this class"?
    pub fn is_bare(&self) -> bool {
        self.surface.is_empty() && self.lemma.is_empty() && self.pre.is_empty() && self.pre_lemma.is_empty() && self.sub.is_none() && !self.title
    }
}

/// (surface, lemma, pre, pre_lemma, sub, title, class, tag, cont, filter, guard, zeros)
type Key<'a> = (String, String, String, String, Option<(char, char)>, bool, ClassId, &'a str, ClassId, u16, u32, &'a [String]);

/// How a stem's letters line up with its lemma's: `s[ps..]` and `l[pl..]`
/// agree for `len` letters, except at `sub` (offset, stem letter, lemma
/// letter), where the stem writes a marked letter.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Alignment {
    ps: usize,
    pl: usize,
    len: usize,
    sub: Option<(usize, char, char)>,
}

/// The same letter: Hebrew final and medial forms match.
fn same(x: char, y: char) -> bool {
    medial(x) == medial(y)
}

fn run(s: &[char], l: &[char], ps: usize, pl: usize, allow_sub: bool) -> Alignment {
    let mut a = Alignment { ps, pl, len: 0, sub: None };
    while let (Some(&x), Some(&y)) = (s.get(ps + a.len), l.get(pl + a.len)) {
        if !same(x, y) {
            if !allow_sub || a.sub.is_some() || x == base_letter(x) || base_letter(x) != y {
                break;
            }
            a.sub = Some((a.len, x, y));
        }
        a.len += 1;
    }
    a
}

/// The alignments explaining the most letters, if they beat the plain
/// common start of `common` letters. Ties are all kept: which one
/// generalizes is for the known stems' support to decide.
fn best_alignments(s: &[char], l: &[char], common: usize) -> Vec<Alignment> {
    let mut top = common;
    let mut best: Vec<Alignment> = Vec::new();
    for a in (0..s.len()).flat_map(|ps| (0..l.len()).flat_map(move |pl| [false, true].map(|m| run(s, l, ps, pl, m)))) {
        if a.len > top {
            top = a.len;
            best.clear();
        }
        if a.len == top && top > common && !best.contains(&a) {
            best.push(a);
        }
    }
    best
}

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
        let mut l: Vec<char> = e.lemma.chars().collect();
        let title = match (s.first(), l.first_mut()) {
            (Some(&a), Some(b)) if b.is_uppercase() && !a.is_uppercase() && b.to_lowercase().eq(std::iter::once(a)) => {
                *b = a;
                true
            }
            _ => false,
        };
        let common = s.iter().zip(&l).take_while(|(a, b)| same(**a, **b)).count();
        let plain = Alignment { ps: 0, pl: 0, len: common, sub: None };
        let changed = if common < s.len() { best_alignments(&s, &l, common) } else { Vec::new() };
        // Alignments can share keys (a plain start and a later letter
        // change); an entry supports each key once.
        let mut keys: BTreeSet<Key> = BTreeSet::new();
        for a in std::iter::once(plain).chain(changed) {
            let pre: String = s.get(..a.ps).unwrap_or_default().iter().collect();
            let pre_lemma: String = l.get(..a.pl).unwrap_or_default().iter().collect();
            // Every ending, the empty one included: the head before it keeps
            // at least one char.
            for start in (1..=a.len).rev() {
                let (Some(st), Some(lt)) = (s.get(a.ps + start..), l.get(a.pl + start..)) else { break };
                let sub = a.sub.filter(|&(at, ..)| at < start);
                // `Shape::lemma_of` changes the head's last marked letter.
                if sub.is_some_and(|(at, x, _)| s.get(a.ps + at + 1..a.ps + start).is_some_and(|rest| rest.contains(&x))) {
                    continue;
                }
                let sub = sub.map(|(_, x, y)| (x, y));
                keys.insert((st.iter().collect(), lt.iter().collect(), pre.clone(), pre_lemma.clone(), sub, title, e.lexicon, e.tag.as_str(), e.cont, e.filter, e.guard, e.zeros.as_slice()));
            }
        }
        for key in keys {
            seen.entry(key).or_insert((id as u32, 0)).1 += 1;
        }
    }
    let mut out: Vec<Shape> = seen
        .into_iter()
        .filter(|(_, (_, support))| *support >= MIN_SUPPORT)
        .map(|((surface, lemma, pre, pre_lemma, sub, title, class, ..), (entry, support))| Shape { surface, lemma, pre, pre_lemma, sub, title, class, entry, support })
        .collect();
    out.sort_by(|a, b| a.surface.cmp(&b.surface).then(b.support.cmp(&a.support)).then(a.entry.cmp(&b.entry)));
    out
}

/// The distinct non-empty prefixes of each class's shapes, sorted.
pub fn prefixes(shapes: &[Shape]) -> Vec<(ClassId, String)> {
    let set: BTreeSet<(ClassId, &str)> = shapes.iter().filter(|s| !s.pre.is_empty()).map(|s| (s.class, s.pre.as_str())).collect();
    set.into_iter().map(|(c, p)| (c, p.to_string())).collect()
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
        w.str(&s.pre);
        w.str(&s.pre_lemma);
        let (from, to) = s.sub.map_or((0, 0), |(a, b)| (a as u32, b as u32));
        w.u32(from);
        w.u32(to);
        w.u8(s.title as u8);
        w.u32(s.class as u32);
        w.u32(s.entry);
        w.u32(s.support);
    }
}

pub fn read(r: &mut Reader, classes: &[ClassRt], entries: &[EntryRt]) -> Result<Vec<Shape>> {
    let n = r.count(37)?;
    let mut out: Vec<Shape> = Vec::with_capacity(n);
    for _ in 0..n {
        let surface = r.string()?;
        let lemma = r.string()?;
        let pre = r.string()?;
        let pre_lemma = r.string()?;
        let sub = match (r.u32()?, r.u32()?) {
            (0, 0) => None,
            (a, b) => match (char::from_u32(a), char::from_u32(b)) {
                (Some(a), Some(b)) if a != '\0' && b != '\0' => Some((a, b)),
                _ => return art_err("bad stem shape letter"),
            },
        };
        let title = match r.u8()? {
            0 => false,
            1 => true,
            _ => return art_err("bad stem shape flag"),
        };
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
        out.push(Shape { surface, lemma, pre, pre_lemma, sub, title, class: class as ClassId, entry, support });
    }
    Ok(out)
}
