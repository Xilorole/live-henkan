/// Quick integration test for hiragana passthrough with real dictionary.
/// Run with: cargo test -p converter --test passthrough_integration
use std::io;
use std::path::Path;

#[test]
fn test_real_dict_passthrough() {
    let dict_dir = Path::new("../../data/dictionary/mecab-ipadic-2.7.0-20070801");
    if !dict_dir.exists() {
        eprintln!("Skipping: dictionary not found at {}", dict_dir.display());
        return;
    }

    let dict = dictionary::Dictionary::load_from_dir(dict_dir).unwrap();
    let matrix_path = dict_dir.join("matrix.def");
    let conn = dictionary::ConnectionCost::from_reader(io::BufReader::new(
        std::fs::File::open(matrix_path).unwrap(),
    ))
    .unwrap();

    // Test: "していて" should stay mostly hiragana, not become "指定て"
    let result = converter::convert_with_conn("していて", &dict, &conn).unwrap();
    let combined: String = result.iter().map(|s| s.surface.as_str()).collect();
    eprintln!("していて -> {combined}");
    assert!(
        !combined.contains("指定"),
        "Expected no 指定, got: {combined}"
    );

    // Test: "にしていて" should not become "二指定て"
    let result2 = converter::convert_with_conn("にしていて", &dict, &conn).unwrap();
    let combined2: String = result2.iter().map(|s| s.surface.as_str()).collect();
    eprintln!("にしていて -> {combined2}");
    assert!(
        !combined2.contains("指定"),
        "Expected no 指定, got: {combined2}"
    );

    // Test: common words still work
    let result3 = converter::convert_with_conn("きょう", &dict, &conn).unwrap();
    let combined3: String = result3.iter().map(|s| s.surface.as_str()).collect();
    eprintln!("きょう -> {combined3}");
    assert_eq!(combined3, "今日");

    // Test: the original problem sentence
    let result4 = converter::convert_with_conn("しんにたのしみにしていて", &dict, &conn).unwrap();
    let combined4: String = result4.iter().map(|s| s.surface.as_str()).collect();
    eprintln!("しんにたのしみにしていて -> {combined4}");
    for seg in &result4 {
        eprintln!(
            "  {:10} reading={:10} cost={:6} L={:4} R={:4}",
            seg.surface, seg.reading, seg.cost, seg.left_id, seg.right_id
        );
    }
    // Should NOT contain 指定
    assert!(
        !combined4.contains("指定"),
        "Expected no 指定, got: {combined4}"
    );

    eprintln!("All passthrough integration tests passed!");
}
