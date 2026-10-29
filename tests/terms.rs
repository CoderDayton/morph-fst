//! Search terms: one call that turns text (a document or a query) into index keys.

use morph_fst::Lexer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn forms_of_a_word_share_one_term() -> TestResult {
    let lx = Lexer::bundled("english")?;
    for form in ["run", "runs", "ran", "running"] {
        assert_eq!(lx.terms(form), ["run"], "{form}");
    }
    assert_eq!(lx.terms("Mice"), lx.terms("mouse"));
    Ok(())
}

#[test]
fn punctuation_symbols_and_clitics_are_not_terms() -> TestResult {
    let lx = Lexer::bundled("english")?;
    assert_eq!(lx.terms("The runners, (running) -- quickly!"), ["the", "runner", "run", "quickly"]);
    assert_eq!(lx.terms("Don't: $5 & more"), ["do", "5", "more"]);
    Ok(())
}

#[test]
fn numbers_emails_urls_and_abbreviations_stay_whole() -> TestResult {
    let lx = Lexer::bundled("english")?;
    assert_eq!(lx.terms("Mail X@Y.com about 4.50 at https://a.b/c, Dr. Who"), ["mail", "x@y.com", "about", "4.50", "at", "https://a.b/c", "dr.", "who"]);
    Ok(())
}

#[test]
fn unknown_words_and_names_get_a_term_too() -> TestResult {
    let lx = Lexer::bundled("english")?;
    assert_eq!(lx.terms("we saw blorfs near Zorbington"), ["we", "see", "blorf", "near", "zorbington"]);
    Ok(())
}

#[test]
fn a_query_and_a_document_meet_in_the_middle() -> TestResult {
    let lx = Lexer::bundled("english")?;
    let doc = lx.terms("The children were running home");
    assert!(lx.terms("child ran").iter().all(|t| doc.contains(t)));
    Ok(())
}
