use super::Fixture;

fn open_window(f: &mut Fixture, c: usize) -> usize {
    let w = f.client_mut(c).create_window();
    f.client_mut(c).commit(w);
    f.roundtrip(c);
    f.client_mut(c).ack_and_commit(w);
    f.roundtrip(c);
    w
}

/// Regression test for batched layer-shell commits (dwlb toggle-visibility).
///
/// When a layer-shell client re-creates its surface, it may batch the
/// initial empty commit and a buffer commit in one socket write. The server
/// processes both before the configure round-trips. Without the pre-set
/// last_acked fix in handle_layer_commit, smithay's pre_commit_hook sees
/// last_acked=None on the buffer commit and posts a protocol error,
/// killing the client.
#[test]
fn batched_initial_commits() {
    let mut f = Fixture::new();
    let c = f.add_client();

    let _w = open_window(&mut f, c);

    // Create layer surface (sets size + anchor, does NOT commit yet).
    let ls = f.client_mut(c).create_layer_surface();

    // Simulate batching: initial empty commit followed by buffer commit,
    // both flushed before the server dispatches.
    f.client_mut(c).layer_commit(ls);
    f.client_mut(c).layer_attach_and_commit(ls);

    // Server processes both commits in one dispatch cycle.
    f.dispatch();

    // If the fix is missing, the server posted a protocol error on the
    // buffer commit, killing the client. A successful roundtrip proves
    // the client survived.
    f.roundtrip(c);
}
