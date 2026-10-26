//! Turkish, Spanish, French, Korean: golden analyses, rule/harmony
//! violations that must fall back, the independent oracle, hybrid ≡ strict
//! on accepted words, and language-specific normalization.

use morph_fst::fused::{derive_vocab, FusedLexer, Mode};
use morph_fst::normalize::{compose_hangul, normalize};
use morph_fst::oracle::{self, Gloss};
use morph_fst::output::{analysis_string, segmentation, WordOutput};
use morph_fst::rng::Rng;
use morph_fst::spec::{self, parse_spec};
use morph_fst::{lex_text, WordLexer};
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LANGS: &[(&str, &str)] = &[
    ("turkish", include_str!("../data/turkish.morph")),
    ("spanish", include_str!("../data/spanish.morph")),
    ("french", include_str!("../data/french.morph")),
    ("korean", include_str!("../data/korean.morph")),
];

fn src(lang: &str) -> Result<&'static str, String> {
    LANGS.iter().find(|(l, _)| *l == lang).map(|(_, s)| *s).ok_or(format!("no spec for {lang}"))
}

fn lexer(lang: &str) -> Result<FusedLexer, Box<dyn std::error::Error>> {
    Ok(FusedLexer::compile(src(lang)?)?)
}

/// (word, expected best analysis, expected segmentation)
fn check_golden(lang: &str, cases: &[(&str, &str, &str)], rejects: &[&str]) -> TestResult {
    let f = lexer(lang)?;
    for &(w, a, seg) in cases {
        let w = normalize(w, &f.mt.norm);
        match f.lex_word(&w) {
            WordOutput::Analyzed { best, .. } => {
                assert_eq!(analysis_string(&best), a, "{lang}: {}", compose_hangul(&w));
                assert_eq!(segmentation(&best), seg, "{lang}: {}", compose_hangul(&w));
            }
            o => panic!("{lang}: {} not analyzed: {}", compose_hangul(&w), o.render()),
        }
    }
    for &w in rejects {
        let w = normalize(w, &f.mt.norm);
        assert!(matches!(f.lex_word(&w), WordOutput::Fallback(_)), "{lang}: {} should fall back", compose_hangul(&w));
    }
    Ok(())
}

#[test]
fn turkish_golden() -> TestResult {
    check_golden(
        "turkish",
        &[
            ("evlerimizden", "ev<N>+lAr<PL>+IxmIz<P1PL>+DAn<ABL>", "ev|ler|imiz|den"),
            ("kitaplarımızdan", "kitap<N>+lAr<PL>+IxmIz<P1PL>+DAn<ABL>", "kitap|lar|ımız|dan"),
            ("kitapta", "kitap<N>+DA<LOC>", "kitap|ta"),
            ("arabaya", "araba<N>+YA<DAT>", "araba|ya"),
            ("saatler", "saat<N>+lAr<PL>", "saat|ler"),
            ("saatte", "saat<N>+DA<LOC>", "saat|te"),
            ("kalpler", "kalp<N>+lAr<PL>", "kalp|ler"),
            ("geliyorum", "gel<V>+Ixyor<PROG>+YIm<1SG>", "gel|iyor|um"),
            ("gelmiyorum", "gel<V>+mA<NEG>+Ixyor<PROG>+YIm<1SG>", "gel|m|iyor|um"),
            ("arıyor", "ara<V>+Ixyor<PROG>", "ar|ıyor"),
            ("okuyor", "oku<V>+Ixyor<PROG>", "oku|yor"),
            ("söylüyor", "söyle<V>+Ixyor<PROG>", "söyl|üyor"),
            ("diyor", "de<V>+Ixyor<PROG>", "d|iyor"),
            ("geleceğim", "gel<V>+YAcAk<FUT>+YIm<1SG>", "gel|eceğ|im"),
            ("gelmeyecekler", "gel<V>+mA<NEG>+YAcAk<FUT>+lAr<3PL>", "gel|me|yecek|ler"),
            ("gidiyor", "git<V>+Ixyor<PROG>", "gid|iyor"),
            ("gitti", "git<V>+DI<PST>", "git|ti"),
            ("yapmışsınız", "yap<V>+mIş<EVID>+sInIz<2PL>", "yap|mış|sınız"),
            ("kitapçı", "kitap<N>+CI<AGT>", "kitap|çı"),
            ("evsizlik", "ev<N>+sIz<PRIV>+lIk<ABST>", "ev|siz|lik"),
        ],
        &["evlerda", "kitapı", "kitapda", "saatlar", "geliyorım", "gelmeyor", "arayor", "kalpları", "gelecekim"],
    )?;
    // Words with several readings: the expected one must be among them.
    let f = lexer("turkish")?;
    for (w, a) in [
        ("kitabı", "kitap<N>+SI<P3SG>"),
        ("kalbi", "kalp<N>+SI<P3SG>"),
        ("rengi", "renk<N>+SI<P3SG>"),
        ("burnu", "burun<N>+SI<P3SG>"),
        ("hakkı", "hak<N>+SI<P3SG>"),
        ("evinde", "ev<N>+SI<P3SG>+nDA<LOC>"),
        ("arabanın", "araba<N>+NIn<GEN>"),
        ("gözlüğü", "göz<N>+lIk<ABST>+SI<P3SG>"),
        ("evleri", "ev<N>+lArI<P3PL>"),
    ] {
        match f.lex_word(w) {
            WordOutput::Analyzed { all, .. } => {
                assert!(all.iter().any(|x| analysis_string(x) == a), "{w}: {:?}", all.iter().map(analysis_string).collect::<Vec<_>>())
            }
            o => panic!("{w}: {}", o.render()),
        }
    }
    Ok(())
}

#[test]
fn spanish_golden() -> TestResult {
    check_golden(
        "spanish",
        &[
            ("canciones", "canción<N>+S<PL>", "cancion|es"),
            ("lápices", "lápiz<N>+S<PL>", "lápic|es"),
            ("luces", "luz<N>+S<PL>", "luc|es"),
            ("países", "país<N>+S<PL>", "país|es"),
            ("sofás", "sofá<N>+S<PL>", "sofá|s"),
            ("piensan", "pensar<V>+an<PRS.3PL>", "piens|an"),
            ("empiece", "empezar<V>+e<SBJV.1/3SG>", "empiec|e"),
            ("empecé", "empezar<V>+é<PRT.1SG>", "empec|é"),
            ("almuerce", "almorzar<V>+e<SBJV.1/3SG>", "almuerc|e"),
            ("busqué", "buscar<V>+é<PRT.1SG>", "busqu|é"),
            ("llegué", "llegar<V>+é<PRT.1SG>", "llegu|é"),
            ("recuerdo", "recordar<V>+o<PRS.1SG>", "recuerd|o"),
            ("vuelvo", "volver<V>+o<PRS.1SG>", "vuelv|o"),
            ("pidió", "pedir<V>+ió<PRT.3SG>", "pid|ió"),
            ("pidiendo", "pedir<V>+iendo<GER>", "pid|iendo"),
            ("imposible", "in<NEG>+posible<ADJ>", "im|posible"),
            ("ilegal", "in<NEG>+legal<ADJ>", "i|legal"),
            ("irreal", "in<NEG>+real<ADJ>", "ir|real"),
            ("casita", "casa<N>+ita<DIM>", "cas|ita"),
            ("rápidamente", "rápido<ADJ>+a<FEM>+mente<ADV>", "rápid|a|mente"),
            ("felices", "feliz<ADJ>+S<PL>", "felic|es"),
            ("ingleses", "inglés<ADJ>+S<PL>", "ingles|es"),
            ("alemanes", "alemán<ADJ>+S<PL>", "aleman|es"),
        ],
        &["cancións", "canciónes", "luzes", "piensamos", "pensan", "empieze", "buscé", "llegé", "inposible", "casaita"],
    )
}

#[test]
fn french_golden() -> TestResult {
    check_golden(
        "french",
        &[
            ("mangeons", "manger<V>+ons<PRS.1PL>", "mange|ons"),
            ("commençons", "commencer<V>+ons<PRS.1PL>", "commenç|ons"),
            ("commençait", "commencer<V>+ait<IPFV.3SG>", "commenç|ait"),
            ("achète", "acheter<V>+e<PRS.1/3SG>", "achèt|e"),
            ("achèterai", "acheter<V>+erai<FUT.1SG>", "achèt|erai"),
            ("préfère", "préférer<V>+e<PRS.1/3SG>", "préfèr|e"),
            ("préférons", "préférer<V>+ons<PRS.1PL>", "préfér|ons"),
            ("appelle", "appeler<V>+e<PRS.1/3SG>", "appell|e"),
            ("appellerai", "appeler<V>+erai<FUT.1SG>", "appell|erai"),
            ("jette", "jeter<V>+e<PRS.1/3SG>", "jett|e"),
            ("chevaux", "cheval<N>+x<PL>", "chevau|x"),
            ("bateaux", "bateau<N>+x<PL>", "bateau|x"),
            ("heureuse", "heureux<ADJ>+e<FEM>", "heureus|e"),
            ("heureusement", "heureux<ADJ>+e<FEM>+ment<ADV>", "heureus|e|ment"),
            ("active", "actif<ADJ>+e<FEM>", "activ|e"),
            ("première", "premier<ADJ>+e<FEM>", "premièr|e"),
            ("bonne", "bon<ADJ>+e<FEM>", "bonn|e"),
            ("naturellement", "naturel<ADJ>+e<FEM>+ment<ADV>", "naturell|e|ment"),
            ("l'homme", "le<DET>+homme<N>", "l'|homme"),
            ("j'aime", "je<PRON>+aimer<V>+e<PRS.1/3SG>", "j'|aim|e"),
            ("d'enfants", "de<PREP>+enfant<N>+s<PL>", "d'|enfant|s"),
            ("finissons", "finir<V>+issons<PRS.1PL>", "fin|issons"),
        ],
        &["commencons", "mangons", "achete", "appele", "chevals", "heureuxe", "bone", "l'maison", "j'parle"],
    )
}

#[test]
fn korean_golden() -> TestResult {
    check_golden(
        "korean",
        &[
            ("학생이", "학생<N>+SUBJ<SUBJ>", "학생|이"),
            ("친구가", "친구<N>+SUBJ<SUBJ>", "친구|가"),
            ("물은", "물<N>+TOP<TOP>", "물|은"),
            ("서울로", "서울<N>+EURO로<DIR>", "서울|로"),
            ("집으로", "집<N>+EURO로<DIR>", "집|으로"),
            ("학교에서는", "학교<N>+에서<LOC.SRC>+TOP<TOP>", "학교|에서|는"),
            ("빵과", "빵<N>+AND<AND>", "빵|과"),
            ("학생이에요", "학생<N>+COP요<COP.POL>", "학생|이에요"),
            ("친구예요", "친구<N>+COP요<COP.POL>", "친구|예요"),
            ("먹어요", "먹<V>+EO요<POL>", "먹|어요"),
            ("가요", "가<V>+EO요<POL>", "가|요"),
            ("갔어요", "가<V>+EOᆻ<PST>+어요<POL>", "가|ᆻ|어요"),
            ("해요", "하<V>+EO요<POL>", "해|요"),
            ("공부해요", "공부하<V>+EO요<POL>", "공부해|요"),
            ("아파요", "아프<V>+EO요<POL>", "아ᄑ|ᅡ요"),
            ("추워요", "춥<V>+EO요<POL>", "추워|요"),
            ("추우면", "춥<V>+EU면<COND>", "추우|면"),
            ("몰라요", "모르<V>+EO요<POL>", "몰ᄅ|ᅡ요"),
            ("들어요", "듣<V>+EO요<POL>", "들|어요"),
            ("들으면", "듣<V>+EU면<COND>", "들|으면"),
            ("먹습니다", "먹<V>+SEUP니다<FORMAL>", "먹|습니다"),
            ("갑니다", "가<V>+SEUP니다<FORMAL>", "가|ᆸ니다"),
            ("와요", "오<V>+EO요<POL>", "와|요"),
            ("만들어요", "만들<V>+EO요<POL>", "만들|어요"),
        ],
        &["학생가", "친구이", "물는", "먹아요", "가아요", "오아요", "하아요", "춥어요", "듣어요", "모르아요", "살세요"],
    )?;
    // optional contractions: both forms are standard
    let f = lexer("korean")?;
    for w in ["봐요", "보아요", "마셔요", "마시어요", "돼요", "되어요", "줘요", "주어요"] {
        let w = normalize(w, &f.mt.norm);
        assert!(matches!(f.lex_word(&w), WordOutput::Analyzed { .. }), "{}", compose_hangul(&w));
    }
    // 사세요: 사다 'buy' and 살다 'live' (ㄹ-deletion) — genuine ambiguity
    match f.lex_word(&normalize("사세요", &f.mt.norm)) {
        WordOutput::Analyzed { all, .. } => {
            let lemmas: BTreeSet<String> = all.iter().map(|a| compose_hangul(&a[0].lemma).into_owned()).collect();
            assert_eq!(lemmas, BTreeSet::from(["사".to_string(), "살".to_string()]));
        }
        o => panic!("{}", o.render()),
    }
    Ok(())
}

/// Independent string-level oracle: the lexer accepts exactly the
/// generated surfaces (≤ 4 morphemes) with exactly those glosses, and reject
/// every wrong archiphoneme choice and every rule-less form not generated.
#[test]
fn oracle_all_languages() -> TestResult {
    const DEPTH: usize = 4;
    let glosses = |out: Option<WordOutput>| -> BTreeSet<Gloss> {
        match out {
            Some(WordOutput::Analyzed { all, .. }) => {
                all.into_iter().filter(|a| a.len() <= DEPTH).map(|a| a.into_iter().map(|m| (m.lemma, m.tag)).collect()).collect()
            }
            _ => BTreeSet::new(),
        }
    };
    for (lang, text) in LANGS {
        let ast = parse_spec(text)?;
        let gen = oracle::generate(&ast, DEPTH, 400_000)?;
        let f = lexer(lang)?;
        for (surface, want) in &gen.surfaces {
            assert_eq!(&glosses(f.analyze(surface, 512)), want, "{lang}: {}", compose_hangul(surface));
        }
        let mut neg = 0;
        for w in gen.naive.iter().chain(&gen.wrong_archi) {
            if gen.surfaces.contains_key(w) {
                continue;
            }
            assert!(glosses(f.analyze(w, 512)).is_empty(), "{lang} accepted forbidden {}", compose_hangul(w));
            neg += 1;
        }
        eprintln!("{lang}: {} surfaces, {} forbidden forms rejected", gen.surfaces.len(), neg);
        assert!(gen.surfaces.len() > 500 && neg > 100, "{lang}: oracle too small");
    }
    Ok(())
}

#[test]
fn hybrid_matches_strict_on_accepted_words() -> TestResult {
    let n: usize = std::env::var("MC_EQUIV_N").ok().and_then(|s| s.parse().ok()).unwrap_or(100_000) / 4;
    for (lang, text) in LANGS {
        let f = lexer(lang)?;
        let good: Vec<String> = oracle::generate(&parse_spec(text)?, 4, 100_000)?.surfaces.into_keys().collect();
        let alpha: Vec<char> = derive_vocab(&spec::load(text)?)
            .iter()
            .filter(|p| !p.cont && p.text.chars().count() == 1)
            .filter_map(|p| p.text.chars().next())
            .chain(['😀', '#', '\''])
            .collect();
        let mut rng = Rng::new(11);
        for i in 0..n {
            let w: String = match i % 3 {
                0 => rng.pick(&good).clone(),
                1 => {
                    let mut cs: Vec<char> = rng.pick(&good).chars().collect();
                    let k = rng.below(cs.len());
                    if rng.chance(1, 2) {
                        cs[k] = *rng.pick(&alpha);
                    } else {
                        cs.remove(k);
                    }
                    cs.into_iter().collect()
                }
                _ => (0..rng.below(14)).map(|_| *rng.pick(&alpha)).collect(),
            };
            let strict = f.lex_word(&w);
            if matches!(strict, WordOutput::Analyzed { .. }) {
                assert_eq!(f.lex(&w, Mode::Hybrid), strict, "{lang}: {}", compose_hangul(&w));
            }
        }
    }
    Ok(())
}

#[test]
fn artifacts_round_trip_and_reject_corruption() -> TestResult {
    let mut rng = Rng::new(3);
    for (lang, text) in LANGS {
        let f = FusedLexer::compile(text)?;
        let bytes = f.to_bytes();
        assert_eq!(bytes, FusedLexer::compile(text)?.to_bytes(), "{lang}: non-deterministic build");
        assert_eq!(FusedLexer::from_bytes(&bytes)?, f);
        for _ in 0..100 {
            let mut bad = bytes.clone();
            let k = rng.below(bad.len());
            bad[k] ^= 1 << rng.below(8);
            assert!(FusedLexer::from_bytes(&bad).is_err());
            assert!(FusedLexer::from_bytes(&bytes[..rng.below(bytes.len())]).is_err());
        }
    }
    Ok(())
}

#[test]
fn normalization_per_language() -> TestResult {
    // Turkish casing: I → ı, İ → i
    let tr = lexer("turkish")?;
    let out = lex_text(&tr, "EVLERİMİZDEN Kitapta");
    assert_eq!(out[0].0, "evlerimizden");
    assert!(matches!(out[0].1, WordOutput::Analyzed { .. }));
    assert!(matches!(out[1].1, WordOutput::Analyzed { .. }));
    // Spanish: decomposed accent (o + U+0301) composes to ó
    let es = lexer("spanish")?;
    let out = lex_text(&es, "Cancio\u{301}n canciones");
    assert_eq!(out[0].0, "canción");
    assert!(out.iter().all(|(_, o)| matches!(o, WordOutput::Analyzed { .. })));
    // French: curly apostrophe kept inside the word
    let fr = lexer("french")?;
    let out = lex_text(&fr, "L’homme mange.");
    assert_eq!(out[0].0, "l'homme");
    assert!(matches!(out[0].1, WordOutput::Analyzed { .. }));
    // Korean: syllables ↔ jamo round trip, words recompose for display
    let ko = lexer("korean")?;
    let out = lex_text(&ko, "학생이 갔어요.");
    assert_eq!(compose_hangul(&out[0].0), "학생이");
    assert_eq!(compose_hangul(&out[1].0), "갔어요");
    assert!(matches!(out[1].1, WordOutput::Analyzed { .. }));
    let mut rng = Rng::new(5);
    for _ in 0..2000 {
        // U+AC00 + [0, 11172) is always a Hangul syllable, so nothing is filtered.
        let s: String = (0..rng.below(8)).filter_map(|_| char::from_u32(0xAC00 + rng.below(11172) as u32)).collect();
        let n = normalize(&s, &ko.mt.norm);
        assert_eq!(compose_hangul(&n), s);
        // lossless fallback over jamo
        if let WordOutput::Fallback(p) = ko.lex_word(&n) {
            assert_eq!(morph_fst::output::reconstruct(&p), n.as_bytes());
        }
    }
    Ok(())
}
