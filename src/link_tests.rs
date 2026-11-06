use super::*;

fn bytes(s: &str) -> [u8; 32] {
    unhex(s).unwrap()
}

// RFC 7748 § 5.2 and § 6.1 vectors, plus ring's ephemeral X25519 as a second implementation: a wrong ladder
// would derive keys no peer shares.
#[test]
fn x25519_matches_rfc_7748_and_ring() {
    assert_eq!(
        hex(&x25519(
            &bytes("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
            &bytes("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c")
        )),
        "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"
    );
    let alice = Key::from(bytes(
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a",
    ));
    let bob = Key::from(bytes(
        "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb",
    ));
    assert_eq!(
        hex(&alice.public),
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    );
    assert_eq!(
        hex(&bob.public),
        "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
    );
    let shared = "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742";
    assert_eq!(
        (
            hex(&x25519(&alice.secret, &bob.public)),
            hex(&x25519(&bob.secret, &alice.public))
        ),
        (shared.into(), shared.into())
    );
    let rng = SystemRandom::new();
    for _ in 0..4 {
        let eph =
            ring::agreement::EphemeralPrivateKey::generate(&ring::agreement::X25519, &rng).unwrap();
        let public: [u8; 32] = eph
            .compute_public_key()
            .unwrap()
            .as_ref()
            .try_into()
            .unwrap();
        let theirs = ring::agreement::agree_ephemeral(
            eph,
            &ring::agreement::UnparsedPublicKey::new(&ring::agreement::X25519, alice.public),
            |k| k.to_vec(),
        )
        .unwrap();
        assert_eq!(x25519(&alice.secret, &public).to_vec(), theirs);
    }
}

// Principle 1: only the addressed key opens a body, a changed byte or a low-order key is refused, and the
// opener learns the sender's key.
#[test]
fn sealed_bodies_open_only_for_their_host_and_name_the_sender() {
    let (a, b, c) = (Key::from([1; 32]), Key::from([2; 32]), Key::from([3; 32]));
    let body = seal(&a, &b.public, b"worker text").unwrap();
    assert!(
        !String::from_utf8_lossy(&STANDARD.decode(&body).unwrap()).contains("worker text"),
        "the relay sees no text"
    );
    assert_eq!(
        open(&b, &body).unwrap(),
        (a.public, b"worker text".to_vec())
    );
    assert!(open(&c, &body).is_err(), "another host cannot read it");
    let mut raw = STANDARD.decode(&body).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    assert!(open(&b, &STANDARD.encode(&raw)).is_err(), "altered");
    raw[last] ^= 1;
    raw[1..33].copy_from_slice(&c.public);
    assert!(
        open(&b, &STANDARD.encode(&raw)).is_err(),
        "a swapped sender key fails authentication"
    );
    assert!(seal(&a, &[0; 32], b"x").is_err(), "low-order key");
    assert_ne!(
        seal(&a, &b.public, b"x").unwrap(),
        seal(&a, &b.public, b"x").unwrap(),
        "fresh salt per message"
    );
}

#[test]
fn key_file_is_stable_and_corruption_is_kept() {
    let install = std::env::temp_dir().join(format!("taskq-link-key-{}", crate::runtime::uuid()));
    let first = key(&install).unwrap().public;
    assert_eq!(key(&install).unwrap().public, first);
    fs::write(install.join("link-key"), "broken").unwrap();
    assert!(key(&install).is_err());
    assert_eq!(
        fs::read_to_string(install.join("link-key")).unwrap(),
        "broken"
    );
    let _ = fs::remove_dir_all(&install);
}

// Principle 2: stale, untrusted (no `link`), malformed and unlisted heartbeats are no peers.
#[test]
fn peers_are_live_heartbeats_of_the_board_and_the_allowlist() {
    let k = hex(&Key::from([4; 32]).public);
    let id = "ab".repeat(32);
    let status = json!({"observed": 1000, "hosts": [
        {"name": "mac", "updated": 990, "link": format!("{id} {k}")},
        {"name": "old", "updated": 600, "link": format!("{id} {k}")},
        {"name": "plain", "updated": 990},
        {"name": "bad", "updated": 990, "link": format!("{id} zz")},
    ]});
    let names = |allow: Option<&[String]>| {
        peers(&status, 300, allow)
            .into_iter()
            .map(|p| p.host)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(None), ["mac"]);
    assert_eq!(names(Some(&["win".into()])), Vec::<String>::new());
    assert_eq!(
        allowed(&json!({"link_hosts": ["mac"]})).unwrap(),
        Some(vec!["mac".to_string()])
    );
    assert!(allowed(&json!({"link_hosts": "mac"})).is_err());
}

// D4 over a 256 KiB relay body: whole records from the cursor side; the far cursor moves to the first record left
// out, so no record is skipped; one oversized record is shortened, never dropped.
#[test]
fn pages_fit_the_relay_bound_without_skipping_records() {
    let big = "x".repeat(40 * 1024);
    let items: Vec<Value> = (0..8)
        .map(|n| json!({"at": n * 100, "i": 0, "role": "assistant", "text": big}))
        .collect();
    let page = json!({"items": items, "start": 0, "end": 800, "size": 900});
    let f = fit(page.clone(), true);
    assert!(f.to_string().len() <= PAGE);
    let kept = f["items"].as_array().unwrap().len();
    assert!(kept > 0 && kept < 8);
    assert_eq!(
        (f["start"].clone(), f["end"].clone()),
        (json!(0), json!(kept * 100))
    );
    let b = fit(page.clone(), false);
    let kept = b["items"].as_array().unwrap().len();
    assert_eq!(
        (b["start"].clone(), b["end"].clone()),
        (json!((8 - kept) * 100), json!(800))
    );
    assert_eq!(b["items"][kept - 1]["at"], 700, "newest kept, in order");
    let one = fit(
        json!({"items": [{"at": 0, "i": 0, "role": "assistant", "text": "y".repeat(400 * 1024)}], "start": 0, "end": 10, "size": 10}),
        true,
    );
    assert!(one.to_string().len() <= PAGE);
    assert_eq!(
        (one["items"][0]["truncated"].clone(), one["end"].clone()),
        (json!(true), json!(10))
    );
    let small = json!({"items": [{"at": 0, "i": 0, "text": "a"}], "start": 0, "end": 5, "size": 5});
    assert_eq!(fit(small.clone(), true), small);
}

// A healthy peer's reply took about 3 s and 11% took over 5 s in Mac telemetry: a reply after 5 s still reads as its page.
#[test]
fn a_reply_slower_than_five_seconds_still_reads_as_its_page() {
    let dir = std::env::temp_dir().join(format!("taskq-link-{}", crate::runtime::uuid()));
    let (viewer_install, worker_install) = (dir.join("mac"), dir.join("win"));
    let (vk, wk) = (key(&viewer_install).unwrap(), key(&worker_install).unwrap());
    let (vid, wid) = ("aa".repeat(32), "bb".repeat(32));
    let relay = Fake::default();
    let viewer = Peer {
        host: "mac".into(),
        installation: vid.clone(),
        key: vk.public,
    };
    let worker = Peer {
        host: "win".into(),
        installation: wid.clone(),
        key: wk.public,
    };
    let read = Read {
        board: "https://example.org/o/r/issues".into(),
        task: 7,
        attempt: 71,
        before: None,
        after: None,
    };
    let (w_relay, v_relay) = (relay.on(&wid), relay.on(&vid));
    let (wi, vi, wkey, vkey) = (
        worker_install.clone(),
        viewer_install.clone(),
        Key::from(wk.secret),
        Key::from(vk.secret),
    );
    let t = std::thread::spawn(move || {
        let (mut w, mut v) = (w_relay, v_relay);
        let until = Instant::now() + Duration::from_secs(7);
        while Instant::now() < until {
            for m in w.recv().unwrap() {
                handle(&wi, &wkey, &m, |_| vec![viewer.clone()], &mut w.clone(), |_| {
                        std::thread::sleep(Duration::from_millis(5500));
                        Ok(json!({"items": [{"at": 0, "i": 0, "role": "assistant", "text": "slow line"}], "start": 0, "end": 1, "size": 1}))
                    }).unwrap();
            }
            for m in v.recv().unwrap() {
                handle(
                    &vi,
                    &vkey,
                    &m,
                    |_| vec![],
                    &mut v.clone(),
                    |_| Err("viewer serves nothing".into()),
                )
                .unwrap();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let page = ask(&viewer_install, &vk, &worker, &read, &mut relay.on(&vid));
    t.join().unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(page.unwrap()["items"][0]["text"], "slow line");
}

/// dashq#234 in memory: per-installation queues; `from` is the authenticated sender.
#[derive(Clone, Default)]
struct Fake(
    std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    String,
);
impl Relay for Fake {
    fn send(&mut self, to: &str, id: &str, body: &str) -> Res<()> {
        if body.len() > BODY {
            return Err("relay send http 413".into());
        }
        self.0
            .lock()
            .unwrap()
            .push((to.into(), json!({"from": self.1, "id": id, "body": body})));
        Ok(())
    }
    fn recv(&mut self) -> Res<Vec<Value>> {
        let mut q = self.0.lock().unwrap();
        let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(to, _)| *to == self.1);
        *q = rest;
        Ok(mine.into_iter().map(|m| m.1).collect())
    }
}
impl Fake {
    fn on(&self, me: &str) -> Self {
        Fake(self.0.clone(), me.into())
    }
}

// A1-A3 against a fake relay: the viewer's read reaches the worker host, which serves only a live peer of its
// board and only the named request; the sealed reply reaches the viewer through its listener. A silent or
// refusing host reads unknown within the wait, never an empty page.
#[test]
fn a_read_crosses_the_fake_relay_sealed_and_fenced() {
    let dir = std::env::temp_dir().join(format!("taskq-link-{}", crate::runtime::uuid()));
    let (viewer_install, worker_install) = (dir.join("mac"), dir.join("win"));
    let (vk, wk, stranger) = (
        key(&viewer_install).unwrap(),
        key(&worker_install).unwrap(),
        Key::from([7; 32]),
    );
    let (vid, wid) = ("aa".repeat(32), "bb".repeat(32));
    let relay = Fake::default();
    let viewer_peer = Peer {
        host: "mac".into(),
        installation: vid.clone(),
        key: vk.public,
    };
    let worker_peer = Peer {
        host: "win".into(),
        installation: wid.clone(),
        key: wk.public,
    };
    let read = Read {
        board: "https://example.org/o/r/issues".into(),
        task: 7,
        attempt: 71,
        before: None,
        after: Some(20),
    };
    let pump = |relay: Fake,
                served: std::sync::Arc<std::sync::Mutex<Vec<Read>>>,
                peers: Vec<Peer>,
                answer: bool| {
        let (wi, vi, wkey, vkey) = (
            worker_install.clone(),
            viewer_install.clone(),
            Key::from(wk.secret),
            Key::from(vk.secret),
        );
        std::thread::spawn(move || {
            let (mut w, mut v) = (relay.on(&"bb".repeat(32)), relay.on(&"aa".repeat(32)));
            let until = Instant::now() + Duration::from_secs(3);
            while Instant::now() < until {
                for m in w.recv().unwrap() {
                    if answer {
                        let _ = handle(
                            &wi,
                            &wkey,
                            &m,
                            |_| peers.clone(),
                            &mut w.clone(),
                            |r| {
                                served.lock().unwrap().push(r.clone());
                                Ok(
                                    json!({"items": [{"at": 20, "i": 0, "role": "assistant", "text": "remote line", "updated": "2026-10-30T22:21:54.044Z"}], "start": 20, "end": 40, "size": 40}),
                                )
                            },
                        );
                    }
                }
                for m in v.recv().unwrap() {
                    let _ = handle(
                        &vi,
                        &vkey,
                        &m,
                        |_| vec![],
                        &mut v.clone(),
                        |_| Err("viewer serves nothing".into()),
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };
    let served = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let t = pump(
        relay.clone(),
        served.clone(),
        vec![viewer_peer.clone()],
        true,
    );
    let began = Instant::now();
    let page = ask(
        &viewer_install,
        &vk,
        &worker_peer,
        &read,
        &mut relay.on(&vid),
    )
    .unwrap();
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "A1: well within 2 s on a fast link"
    );
    assert_eq!(page["items"][0]["text"], "remote line");
    assert_eq!(
        page["items"][0]["updated"], "2026-10-30T22:21:54.044Z",
        "#1930 A7: encrypted remote reads retain source time verbatim"
    );
    assert_eq!(
        *served.lock().unwrap(),
        vec![read.clone()],
        "A2: the server sees exactly the named attempt and cursor"
    );
    assert!(
        fs::read_dir(viewer_install.join("link"))
            .unwrap()
            .next()
            .is_none(),
        "principle 4: nothing kept"
    );
    t.join().unwrap();

    // a host that is not a live peer of the board gets nothing, and the viewer reads unknown
    let served = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let t = pump(
        relay.clone(),
        served.clone(),
        vec![Peer {
            key: stranger.public,
            ..viewer_peer.clone()
        }],
        true,
    );
    let began = Instant::now();
    let e = ask_within(
        &viewer_install,
        &vk,
        &worker_peer,
        &read,
        &mut relay.on(&vid),
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(
        e.contains("did not answer within 1 s") && began.elapsed() >= Duration::from_secs(1),
        "A3: {e}"
    );
    assert!(served.lock().unwrap().is_empty(), "principle 2");
    t.join().unwrap();

    // a forged `from` with a peer's key and a reply the viewer did not ask for are dropped
    let mut w = relay.on(&wid);
    let forged = json!({"from": "cc".repeat(32), "id": "11".repeat(16), "body": seal(&vk, &wk.public, json!({"kind": "read", "id": "11".repeat(16), "board": "b", "task": 1, "attempt": 1, "before": null, "after": null}).to_string().as_bytes()).unwrap()});
    assert!(
        handle(
            &worker_install,
            &wk,
            &forged,
            |_| vec![viewer_peer.clone()],
            &mut w,
            |_| panic!("served a forged sender")
        )
        .is_err()
    );
    let stray = json!({"from": wid, "id": "22".repeat(16), "body": seal(&wk, &vk.public, json!({"kind": "page", "id": "22".repeat(16)}).to_string().as_bytes()).unwrap()});
    handle(
        &viewer_install,
        &vk,
        &stray,
        |_| vec![],
        &mut relay.on(&vid),
        |_| unreachable!(),
    )
    .unwrap();
    assert!(
        !viewer_install
            .join("link")
            .join(format!("{}.reply", "22".repeat(16)))
            .exists(),
        "no viewer waits: not kept"
    );
    let _ = fs::remove_dir_all(&dir);
}

// The client spoke an unversioned body that dashq 0.18.0 refuses with 400 unsupported_protocol; its own tests
// send these shapes.
#[test]
fn relay_requests_match_dashq_0_18() {
    assert_eq!(
        send_request("host-b", "m1", "AA=="),
        json!({"protocol": 1, "to": "host-b", "id": "m1", "body": "AA=="})
    );
    assert_eq!(recv_request(), json!({"protocol": 1, "wait": 25}));
    assert_eq!(
        refused("send", 403, &json!({"protocol": 1, "error": "scope"})),
        "relay send http 403 scope"
    );
}

// Live relay probe (dashq#234): on a host with an active DashQ connection, run
// `TASKQ_LINK_LIVE=<install> cargo test --locked --bin taskq link::tests::live_relay_round_trip -- --ignored --nocapture`.
// One sealed read goes to this installation through the real relay and its page comes back; it prints the round
// trip, the host link delay that A1 adds to the 2 s panel bound. A running service of this build on that host
// takes over the receive, so stop it first.
#[test]
#[ignore]
fn live_relay_round_trip() {
    let live = PathBuf::from(
        std::env::var("TASKQ_LINK_LIVE").expect("TASKQ_LINK_LIVE names an enrolled install folder"),
    );
    let spool_dir =
        std::env::temp_dir().join(format!("taskq-link-live-{}", crate::runtime::uuid()));
    let mut k = [0u8; 32];
    SystemRandom::new().fill(&mut k).unwrap();
    let me = Dash::new(&live).unwrap().1;
    let peer = Peer {
        host: "self".into(),
        installation: me.clone(),
        key: Key::from(k).public,
    };
    let read = Read {
        board: "live-probe".into(),
        task: 1,
        attempt: 1,
        before: None,
        after: None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let listener = {
        let (live, dir, peer, stop) = (live.clone(), spool_dir.clone(), peer.clone(), stop.clone());
        std::thread::spawn(move || {
            let (mut relay, key) = (Dash::new(&live).unwrap().0, Key::from(k));
            while !stop.load(Ordering::Relaxed) {
                for m in relay.recv().unwrap() {
                    let served = handle(
                        &dir,
                        &key,
                        &m,
                        |_| vec![peer.clone()],
                        &mut Dash::new(&live).unwrap().0,
                        |r| {
                            Ok(
                                json!({"items": [{"at": 0, "i": 0, "role": "assistant", "text": format!("live probe #{}", r.task)}], "start": 0, "end": 1, "size": 1}),
                            )
                        },
                    );
                    if let Err(e) = served {
                        println!("dropped: {e}")
                    }
                }
            }
        })
    };
    std::thread::sleep(Duration::from_secs(1));
    let mut rounds = vec![];
    for _ in 0..5 {
        let began = Instant::now();
        let page = ask(
            &spool_dir,
            &Key::from(k),
            &peer,
            &read,
            &mut Dash::new(&live).unwrap().0,
        )
        .unwrap();
        rounds.push(began.elapsed().as_millis());
        assert_eq!(page["items"][0]["text"], "live probe #1");
        std::thread::sleep(Duration::from_secs(1));
    }
    stop.store(true, Ordering::Relaxed);
    println!("host link round trips through the relay (ms): {rounds:?}");
    assert!(rounds.iter().all(|r| *r < WAIT.as_millis()));
    listener.join().unwrap();
    let _ = fs::remove_dir_all(&spool_dir);
}
