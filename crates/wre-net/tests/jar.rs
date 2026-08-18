use wre_net::jar::Jar;

#[test]
fn the_header_is_ordered_the_way_a_browser_writes_it() {
    let jar = Jar::new();

    jar.add("https://example.org/", "session=1; Path=/").unwrap();
    jar.add("https://example.org/", "token=2; Path=/").unwrap();
    jar.add("https://example.org/inner/", "deep=3; Path=/inner").unwrap();
    jar.add("https://example.org/", "abck=4; Path=/").unwrap();
    jar.add("https://example.org/", "token=5; Path=/").unwrap();

    assert_eq!(
        jar.header("https://example.org/inner/page"),
        "deep=3; session=1; token=5; abck=4"
    );
}
