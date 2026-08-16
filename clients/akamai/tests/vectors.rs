use serde_json::json;

use wre_client_akamai::pow;

#[test]
fn the_search_answers_the_same_nonces_every_time() {
    let challenge = pow::from_abck("TOKEN~-1~salt~-1~3-abcd-500-1000-30-2").remove(0);
    let first = pow::solve_rounds(&challenge, 1_760_000_000_000, 3, 5_000_000, 4).unwrap();
    let second = pow::solve_rounds(&challenge, 1_760_000_000_000, 3, 5_000_000, 1).unwrap();

    assert_eq!(first.nonces, second.nonces);
    assert!(pow::verify(&challenge, 1_760_000_000_000, &first.nonces).unwrap());

    println!("nonces {}", json!(first.nonces));
    println!("prefix {}", first.prefix);
}
