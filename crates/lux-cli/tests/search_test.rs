use agent_lx_music::cmd::search::parse_search_directives;

#[test]
fn test_parse_search_directives() {
    let d = parse_search_directives("Yesterday artist:\"The Beatles\" album:Help kbps:320");
    assert_eq!(d.query, "Yesterday");
    assert_eq!(d.artist.as_deref(), Some("The Beatles"));
    assert_eq!(d.album.as_deref(), Some("Help"));
    assert_eq!(d.kbps.as_deref(), Some("320"));

    let d2 = parse_search_directives("Yesterday artist:\"The Beatles\"");
    assert_eq!(d2.query, "Yesterday");
    assert_eq!(d2.artist.as_deref(), Some("The Beatles"));

    let d3 = parse_search_directives("Yesterday");
    assert_eq!(d3.query, "Yesterday");
    assert_eq!(d3.artist, None);
    assert_eq!(d3.album, None);
    assert_eq!(d3.kbps, None);

    let d4 = parse_search_directives("Yesterday artist：\"The Beatles\" album：Help");
    assert_eq!(d4.query, "Yesterday");
    assert_eq!(d4.artist.as_deref(), Some("The Beatles"));
    assert_eq!(d4.album.as_deref(), Some("Help"));
}
