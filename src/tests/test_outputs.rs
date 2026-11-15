use super::{Fixture, client::OutputState};

fn summary(outputs: &[OutputState]) -> Vec<(&str, (i32, i32))> {
    outputs.iter().map(|o| (o.name.as_str(), o.mode)).collect()
}

#[test]
fn client_sees_each_output_once() {
    let mut f = Fixture::new();
    f.add_output("second", (1920, 1080));
    let c = f.add_client();
    f.roundtrip(c);

    assert_eq!(
        summary(f.client(c).outputs()),
        [("test", (1000, 800)), ("second", (1920, 1080))]
    );
}

#[test]
fn output_added_later_is_announced_once() {
    let mut f = Fixture::new();
    let c = f.add_client();
    f.roundtrip(c);
    assert_eq!(summary(f.client(c).outputs()), [("test", (1000, 800))]);

    f.add_output("second", (1920, 1080));
    f.roundtrip(c);
    f.roundtrip(c);

    assert_eq!(
        summary(f.client(c).outputs()),
        [("test", (1000, 800)), ("second", (1920, 1080))]
    );
}
