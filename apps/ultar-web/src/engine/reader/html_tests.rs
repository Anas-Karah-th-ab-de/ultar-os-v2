use super::*;

#[test]
fn blocks_links_title_and_hidden_parts() {
    let html = r#"<html><head><title> سوريا &amp; العالم </title><style>p{}</style>
        <script>alert("x<p>")</script></head><body>
        <h1>عنوان</h1><p>نص <a href="/wiki/دمشق">دمشق</a> هنا.</p>
        <ul><li>one</li><li><a href='https://example.org/x'>two</a></li></ul>
        <pre>a  b
c</pre><!-- <p>no</p> --><img src=x alt="صورة"></body></html>"#;
    let d = read(html, "https://ar.wikipedia.org/wiki/سوريا");
    assert_eq!(d.title, "سوريا & العالم");
    let texts: Vec<&str> = d.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(
        texts,
        ["عنوان", "نص دمشق هنا.", "one", "two", "a  b\nc", "[صورة]"]
    );
    assert_eq!(d.blocks[0].kind, Kind::Heading(1));
    assert_eq!(d.blocks[2].kind, Kind::Item);
    assert_eq!(d.blocks[4].kind, Kind::Pre);
    let link = &d.blocks[1].links[0];
    assert_eq!(&d.blocks[1].text[link.start..link.end], "دمشق");
    assert_eq!(link.href, "https://ar.wikipedia.org/wiki/دمشق");
    assert_eq!(d.blocks[3].links[0].href, "https://example.org/x");
}

#[test]
fn video_and_audio_become_media_blocks() {
    let html = r#"<p>قبل</p><video controls poster="p.jpg"><source src="clip.webm" type="video/webm">
        <source src="/v/clip.mp4" type="video/mp4">متصفحك لا يدعم الفيديو</video>
        <audio src="https://cdn.org/song%20one.mp3" title="أغنية"></audio><p>بعد</p>"#;
    let d = read(html, "https://h.org/page/");
    let kinds: Vec<Kind> = d.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            Kind::Para,
            Kind::Media(MediaKind::Video),
            Kind::Media(MediaKind::Audio),
            Kind::Para
        ]
    );
    assert_eq!(d.blocks[1].media_src(), Some("https://h.org/v/clip.mp4"));
    assert_eq!(d.blocks[1].text, "clip.mp4");
    assert_eq!(d.blocks[2].text, "أغنية");
    assert_eq!(
        d.blocks[2].media_src(),
        Some("https://cdn.org/song%20one.mp3")
    );
    assert_eq!(d.blocks[3].text, "بعد");
}

#[test]
fn broken_markup_does_not_panic() {
    for html in [
        "<",
        "<a href='x",
        "<p>a<b",
        "&#xFFFFFFF;",
        "<!--",
        "<a href=>x</a>",
        "&",
    ] {
        let _ = read(html, "http://h/");
    }
    assert_eq!(entities("&lt;&#1587;&#x0645;&bogus;"), "<سم&bogus;");
}

#[test]
fn relative_links() {
    let b = "https://h.org/a/b/c.html?q=1";
    assert_eq!(
        join(b, "d.html").as_deref(),
        Some("https://h.org/a/b/d.html")
    );
    assert_eq!(join(b, "../d").as_deref(), Some("https://h.org/a/d"));
    assert_eq!(join(b, "/x?y").as_deref(), Some("https://h.org/x?y"));
    assert_eq!(join(b, "//cdn.org/z").as_deref(), Some("https://cdn.org/z"));
    assert_eq!(
        join(b, "?p=2").as_deref(),
        Some("https://h.org/a/b/c.html?p=2")
    );
    assert_eq!(join(b, "javascript:alert(1)"), None);
    assert_eq!(join(b, "mailto:a@b"), None);
    assert_eq!(join(b, "#top"), None);
}
