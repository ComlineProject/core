/*
// Standard Uses

// Crate Uses
use comline::schema::ir::frozen::blob;
use comline::schema::ir::frozen::blob::FrozenBlob;

// External Uses



#[test]
pub fn frozen_nodes_into_processed() {
    let nodes = vec![
      FrozenBlob::Content("Hello blob".to_owned())
    ];

    let (hash, processed) = blob::to_processed(nodes);

    assert_eq!(hash, "1f8473854dc9445b9c55a16202fb191e4b7b969e5521f32a21d884c31d413335");
    assert_eq!(
        processed,
        vec![
            18, 0, 0, 0, 240, 3, 98, 108, 111, 98, 32,
            49, 48, 32, 72, 101, 108, 108, 111, 32, 98, 108, 111, 98
        ]
    );
}


#[test]
pub fn frozen_nodes_from_processed() {
    let (hash, processed) = (
        "1f8473854dc9445b9c55a16202fb191e4b7b969e5521f32a21d884c31d413335".to_owned(),
        vec![
            18, 0, 0, 0, 240, 3, 98, 108, 111, 98, 32,
            49, 48, 32, 72, 101, 108, 108, 111, 32, 98, 108, 111, 98
        ]
    );

    let nodes = blob::from_processed(hash, processed).unwrap();

    assert_eq!(nodes, vec![FrozenBlob::Content("Hello blob".to_owned())]);
}
*/
