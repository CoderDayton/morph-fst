mod common;
use common::TestResult;
use morph_fst::fused::Mode;
use morph_fst::output::{analysis_string, pieces_string, segmentation, WordOutput};
use morph_fst::WordLexer;

fn best(out: &WordOutput) -> Option<String> {
    match out {
        WordOutput::Analyzed { best, .. } => Some(analysis_string(best)),
        _ => None,
    }
}

fn fallback(out: &WordOutput) -> Option<String> {
    match out {
        WordOutput::Fallback(p) => Some(pieces_string(p)),
        _ => None,
    }
}

const ANALYZED: &[(&str, &str, &str)] = &[
    ("unhappiness", "un<PFX:NEG>+happy<ADJ>+ness<NMLZ>", "un|happi|ness"),
    ("happily", "happy<ADJ>+ly<ADV>", "happi|ly"),
    ("making", "make<V>+ing<PROG>", "mak|ing"),
    ("stopped", "stop<V>+ed<PAST>", "stopp|ed"),
    ("carrying", "carry<V>+ing<PROG>", "carry|ing"),
    ("carried", "carry<V>+ed<PAST>", "carri|ed"),
    ("carries", "carry<V>+s<3SG>", "carrie|s"),
    ("reruns", "re<PFX:AGAIN>+run<V>+s<3SG>", "re|run|s"),
    ("rerunning", "re<PFX:AGAIN>+run<V>+ing<PROG>", "re|runn|ing"),
    ("sleepiness", "sleep<N>+y<ADJZ:Y>+ness<NMLZ>", "sleep|i|ness"),
    ("friendliness", "friend<N>+ly<ADJZ:LY>+ness<NMLZ>", "friend|li|ness"),
    ("unbelievably", "un<PFX:REV>+believe<V>+able<ABLE>+ly<ADV>", "un|believ|ab|ly"),
    ("unstoppable", "un<PFX:REV>+stop<V>+able<ABLE>", "un|stopp|able"),
    ("happinesses", "happy<ADJ>+ness<NMLZ>+s<PL>", "happi|nesse|s"),
    ("business", "busy<ADJ>+ness<NMLZ>", "busi|ness"),
    ("cities", "city<N>+s<PL>", "citie|s"),
    ("boxes", "box<N>+s<PL>", "boxe|s"),
    ("churches", "church<N>+s<PL>", "churche|s"),
    ("creation", "create<V>+ion<NMLZ>", "creat|ion"),
    ("natural", "nature<N>+al<ADJZ:AL>", "natur|al"),
    ("beautifully", "beauty<N>+ful<ADJZ:FULL>+ly<ADV>", "beauti|ful|ly"),
    ("sunny", "sun<N>+y<ADJZ:Y>", "sunn|y"),
    ("sadden", "sad<ADJ>+en<VBZ>", "sadd|en"),
    ("children", "child<N+PL>", "children"),
    ("ownership", "own<V>+er<AGENT>+ship<NMLZ:STATE>", "own|er|ship"),
    ("unemployment", "un<PFX:REV>+employ<V>+ment<NMLZ>", "un|employ|ment"),
];

/// Words the rules make ungrammatical, typos and neologisms → fallback.
const FALLBACK: &[(&str, &str)] = &[
    ("unhappyness", "un ##happy ##ness"),
    ("happyness", "happy ##ness"),
    ("carriing", "carri ##ing"),
    ("carryed", "carry ##ed"),
    ("makeing", "make ##ing"),
    ("stoped", "stop ##ed"),
    ("citys", "city ##s"),
    ("blorptastic", "bl ##o ##rp ##tastic"),
    ("😀", "<0xF0> <0x9F> <0x98> <0x80>"),
    ("漢字", "<0xE6> <0xBC> <0xA2> <0xE5> <0xAD> <0x97>"),
    ("", ""),
];

#[test]
fn golden_analyses() -> TestResult {
    let f = common::fused()?;
    for &(w, a, seg) in ANALYZED {
        let out = f.lex_word(w);
        assert_eq!(best(&out).as_deref(), Some(a), "{w}");
        if let WordOutput::Analyzed { best, .. } = &out {
            assert_eq!(segmentation(best), seg, "{w}");
        }
    }
    Ok(())
}

#[test]
fn golden_fallbacks() -> TestResult {
    let f = common::fused()?;
    for &(w, p) in FALLBACK {
        assert_eq!(fallback(&f.lex_word(w)).as_deref(), Some(p), "{w}");
    }
    Ok(())
}

#[test]
fn ambiguity_is_reported_deterministically() -> TestResult {
    let f = common::fused()?;
    let all = |w: &str| match f.lex_word(w) {
        WordOutput::Analyzed { all, .. } => all.iter().map(analysis_string).collect::<Vec<_>>(),
        o => panic!("{w}: {o:?}"),
    };
    assert_eq!(all("walks"), ["walk<N>+s<PL>", "walk<V>+s<3SG>"]);
    assert_eq!(all("cleaner"), ["clean<V>+er<AGENT>", "clean<ADJ>+er<CMP>"]);
    assert_eq!(all("luckily"), ["lucky<ADJ>+ly<ADV>", "luck<N>+y<ADJZ:Y>+ly<ADV>"]);
    Ok(())
}

#[test]
fn hybrid_mode() -> TestResult {
    let f = common::fused()?;
    let h = |w: &str| f.lex(w, Mode::Hybrid).render();
    assert_eq!(h("unhappyness"), "un<PFX:NEG> + ⟨happy⟩<?ADJ>{happy} + ness<NMLZ>");
    assert_eq!(h("carryed"), "⟨carry⟩<?V>{carry} + ed<PAST>");
    assert_eq!(h("unfrobnicated"), "un<PFX:REV> + ⟨frobnicat⟩<?V>{fr ##o ##b ##nic ##a ##t} + ed<PAST>");
    // No known morpheme anywhere → identical to strict fallback.
    assert_eq!(f.lex("blorptastic", Mode::Hybrid), f.lex("blorptastic", Mode::Strict));
    assert_eq!(f.lex("😀", Mode::Hybrid), f.lex("😀", Mode::Strict));
    // Hybrid never changes a word the FST accepts.
    for &(w, ..) in ANALYZED {
        assert_eq!(f.lex(w, Mode::Hybrid), f.lex(w, Mode::Strict), "{w}");
    }
    Ok(())
}

#[test]
fn text_level_lexing() -> TestResult {
    let f = common::fused()?;
    let out = morph_fst::lex_text(&f, "The Unhappiest children, rerunning tests!");
    let words: Vec<&str> = out.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(words, ["the", "unhappiest", "children", ",", "rerunning", "tests", "!"]);
    assert_eq!(out[1].1.source(), "FST");
    assert_eq!(out[3].1.source(), "SUB");
    Ok(())
}
