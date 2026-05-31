use agent_lx_music::library::playlist_parser::{
    ImportedTrack, calculate_agent_score, parse_csv, parse_m3u, parse_txt,
};
use lux_core::types::{MusicInfo, Source};

#[test]
fn test_playlist_parsers() {
    // 1. M3U Parser
    let m3u_data = "#EXTM3U\n#EXTINF:260,ArtistA - SongA\n/local/path.mp3";
    let tracks = parse_m3u(m3u_data);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "SongA");
    assert_eq!(tracks[0].artist, "ArtistA");

    // 2. CSV Parser
    let csv_data = "Track Name,Artist Name(s),Album\nSongB,ArtistB,AlbumB";
    let tracks = parse_csv(csv_data);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "SongB");
    assert_eq!(tracks[0].artist, "ArtistB");

    // 3. Plain Text Parser
    let txt_data = "ArtistC - SongC\n# Comment line\nSongD - ArtistD";
    let tracks = parse_txt(txt_data);
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title, "SongC");
    assert_eq!(tracks[0].artist, "ArtistC");
}

#[test]
fn test_scoring_weights() {
    let candidate = MusicInfo {
        songmid: "123".to_string(),
        name: "Sunny Day".to_string(),
        singer: "Jay Chou".to_string(),
        source: Source::NetEase,
        album_name: Some("Ye Hui Mei".to_string()),
        album_id: None,
        interval: None,
        pic_url: None,
        hash: None,
        extra: None,
    };

    let target = ImportedTrack {
        title: "Sunny Day".to_string(),
        artist: "Jay Chou".to_string(),
        album: Some("Ye Hui Mei".to_string()),
        song_id: None,
        source: None,
    };

    let score = calculate_agent_score(
        &candidate,
        &target,
        &["wy".to_string(), "kw".to_string()],
        None,
    );
    assert!(score > 80.0);
}
