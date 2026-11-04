use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Notify;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "yuki-rust-writer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
}
impl Gate {
    async fn hold(&self) {
        self.entered.notify_one();
        self.release.notified().await;
    }
    async fn reached(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.entered.notified())
            .await
            .unwrap();
    }
}

struct FaultDriver {
    inner: SqliteDriver,
    apply_gate: Option<Arc<Gate>>,
    commit_gate: Option<Arc<Gate>>,
    close_gate: Option<Arc<Gate>>,
    fail_rollback: bool,
    fail_close: bool,
}
impl Driver for FaultDriver {
    async fn begin(&mut self) -> Result<()> {
        self.inner.begin().await
    }
    async fn apply(&mut self, action: &Mutation) -> Result<serde_json::Value> {
        let result = self.inner.apply(action).await;
        if let Some(gate) = self.apply_gate.take() {
            gate.hold().await;
        }
        result
    }
    async fn commit(&mut self) -> Result<()> {
        if let Some(gate) = self.commit_gate.take() {
            gate.hold().await;
        }
        self.inner.commit().await
    }
    async fn rollback(&mut self) -> Result<()> {
        if self.fail_rollback {
            Err("injected rollback failure".into())
        } else {
            self.inner.rollback().await
        }
    }
    async fn close(&mut self) -> Result<()> {
        if let Some(gate) = self.close_gate.take() {
            gate.hold().await;
        }
        if self.fail_close {
            Err("injected unknown physical close".into())
        } else {
            self.inner.close().await
        }
    }
    async fn reopen(&mut self) -> Result<()> {
        self.inner.reopen().await
    }
}

async fn fixture() -> (Temp, SqliteDriver, SqlitePool) {
    let temp = Temp::new();
    bootstrap(&temp.0).await.unwrap();
    let driver = SqliteDriver::open(&temp.0).await.unwrap();
    let pool = read_pool(&temp.0).await.unwrap();
    (temp, driver, pool)
}

#[tokio::test]
async fn r01_cancel_rollback_failure_keeps_writer_until_physical_close() {
    let (_temp, inner, pool) = fixture().await;
    let apply = Arc::new(Gate::default());
    let close = Arc::new(Gate::default());
    let driver = FaultDriver {
        inner,
        apply_gate: Some(apply.clone()),
        commit_gate: None,
        close_gate: Some(close.clone()),
        fail_rollback: true,
        fail_close: false,
    };
    let store = Store::start(driver, pool);
    let mut hints = store.signals.hints.subscribe();
    let caller_store = store.clone();
    let caller = tokio::spawn(async move {
        caller_store
            .write(Mutation::SetLabel("must rollback".into()))
            .await
    });
    apply.reached().await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    apply.release.notify_one();
    close.reached().await;
    let health = store.health().await;
    assert!(
        health.reader_ok,
        "a working reader does not certify writer cleanup"
    );
    assert!(!health.writer_accepting);
    assert!(
        store
            .write(Mutation::SetLabel("too early".into()))
            .await
            .is_err()
    );
    assert!(hints.try_recv().is_err());
    close.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !store.signals.accepting.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(store.label().await.unwrap().is_null());
    store
        .write(Mutation::SetLabel("next writer".into()))
        .await
        .unwrap();
    assert_eq!(hints.recv().await.unwrap(), 1);
    assert!(hints.try_recv().is_err());
    assert_eq!(store.health().await.commits, 1);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn r01_unknown_close_stops_new_writers_while_reader_works() {
    let (_temp, inner, pool) = fixture().await;
    let store = Store::start(
        FaultDriver {
            inner,
            apply_gate: None,
            commit_gate: None,
            close_gate: None,
            fail_rollback: true,
            fail_close: true,
        },
        pool,
    );
    assert!(store.write(Mutation::Invalid).await.is_err());
    let health = store.health().await;
    assert!(health.reader_ok);
    assert!(!health.writer_accepting);
    assert!(
        store
            .write(Mutation::SetLabel("forbidden".into()))
            .await
            .is_err()
    );
    assert_eq!(health.commits, 0);
    assert!(store.shutdown().await.is_err());
}

#[tokio::test]
async fn root_commit_hints_exclude_savepoint_release_and_rollback() {
    let (_temp, inner, pool) = fixture().await;
    let commit = Arc::new(Gate::default());
    let store = Store::start(
        FaultDriver {
            inner,
            apply_gate: None,
            commit_gate: Some(commit.clone()),
            close_gate: None,
            fail_rollback: false,
            fail_close: false,
        },
        pool,
    );
    let mut hints = store.signals.hints.subscribe();
    let caller_store = store.clone();
    let caller = tokio::spawn(async move {
        caller_store
            .write(Mutation::SetLabel("committed".into()))
            .await
    });
    commit.reached().await;
    assert!(
        hints.try_recv().is_err(),
        "savepoint release is not a root commit"
    );
    assert!(store.label().await.unwrap().is_null());
    commit.release.notify_one();
    caller.await.unwrap().unwrap();
    assert_eq!(hints.recv().await.unwrap(), 1);
    assert!(
        !store.signals.active.load(Ordering::SeqCst),
        "hint follows writer release"
    );
    assert!(store.write(Mutation::Invalid).await.is_err());
    assert!(hints.try_recv().is_err(), "rollback emits no commit hint");
    assert_eq!(store.label().await.unwrap()["value"], "committed");
    store.shutdown().await.unwrap();
}

pub(super) async fn commit_barrier(phase: &str) {
    if std::env::var("YUKI_WRITER_TEST_PHASE").as_deref() == Ok(phase) {
        std::fs::write(std::env::var_os("YUKI_WRITER_TEST_MARKER").unwrap(), phase).unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
        panic!("parent failed to terminate child at {phase} commit");
    }
}

#[tokio::test]
#[ignore = "launched and killed in both phases by the CORE-008 case"]
async fn ingress_crash_child() {
    let data = PathBuf::from(std::env::var_os("YUKI_WRITER_TEST_DATA").unwrap());
    let _owner = crate::config::Owner::acquire(&data).unwrap();
    let store = Store::open(&data).await.unwrap();
    let frame = serde_json::from_str(&std::env::var("YUKI_INGRESS_TEST_FRAME").unwrap()).unwrap();
    crate::ingress::admit::record(&store, frame, true)
        .await
        .unwrap();
    panic!("an ACK was returned before the parent killed the child");
}

#[tokio::test]
#[ignore = "launched under the CORE-010 crash harness"]
async fn effect_crash_child() {
    let data = PathBuf::from(std::env::var_os("YUKI_WRITER_TEST_DATA").unwrap());
    let _owner = crate::config::Owner::acquire(&data).unwrap();
    let store = Store::open(&data).await.unwrap();
    let command = serde_json::from_str(&std::env::var("YUKI_EFFECT_TEST_ACTION").unwrap()).unwrap();
    crate::runtime::effects::invoke(&store, command)
        .await
        .unwrap();
    panic!("receipt returned before simulated reply loss");
}

#[tokio::test]
#[ignore = "launched under the CORE-052 crash harness"]
async fn privacy_crash_child() {
    let data = PathBuf::from(std::env::var_os("YUKI_WRITER_TEST_DATA").unwrap());
    let _owner = crate::config::Owner::acquire(&data).unwrap();
    let store = Store::open(&data).await.unwrap();
    let command = serde_json::from_str(&std::env::var("YUKI_ERASE_TEST").unwrap()).unwrap();
    crate::privacy::invoke(&store, command).await.unwrap();
    panic!("receipt returned before simulated reply loss");
}

#[tokio::test]
#[ignore = "launched under the CORE-013 crash harness"]
async fn social_crash_child() {
    let data = PathBuf::from(std::env::var_os("YUKI_WRITER_TEST_DATA").unwrap());
    let _owner = crate::config::Owner::acquire(&data).unwrap();
    let store = Store::open(&data).await.unwrap();
    let [exec, id, actor]: [String; 3] =
        serde_json::from_str(&std::env::var("YUKI_SOCIAL_TEST").unwrap()).unwrap();
    store
        .write(Mutation::Social(Box::new(crate::social::Change::Next(
            exec,
            id,
            actor,
            None,
            crate::social::split::Limits::default(),
        ))))
        .await
        .unwrap();
    panic!("receipt returned before simulated reply loss");
}

#[tokio::test]
async fn history_drops_projection_prepared_before_source_erasure() {
    let (_temp, mut driver, readers) = fixture().await;
    let conn = driver.connection();
    sqlx::raw_sql("INSERT INTO persons(id) VALUES('00000000-0000-4000-8000-000000000088');
      INSERT INTO accounts VALUES('a','qq','history','1'),('b','qq','history','2');
      INSERT INTO assignments(id,account_id,original_person,kind,status,first_observed) VALUES('human','a','00000000-0000-4000-8000-000000000088','person','active',1),('self','b',NULL,'self','active',1);
      INSERT INTO entries(id,scope,actor_stage,bot_stage) VALUES('private','private','human','self');
      INSERT INTO events(id,source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,bot_stage,observed_at,envelope,mentions) VALUES(1,'scope','key','private','a','00000000-0000-4000-8000-000000000088','human','self',1,'{\"kind\":\"message\",\"text\":\"canary\"}','[]');")
      .execute(&mut *conn).await.unwrap();
    let id: i64 = sqlx::query_scalar("SELECT id FROM history_items WHERE source_event=1")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let pending = crate::history::index::Prepared {
        id,
        revision: 1,
        fields: vec![],
        terms: "private-index-canary".into(),
    };
    crate::privacy::apply(
        conn,
        &crate::privacy::Action::Erase {
            key: "forget".into(),
            person: "00000000-0000-4000-8000-000000000088".into(),
            sources: "[1]".into(),
        },
    )
    .await
    .unwrap();
    let replay = crate::history::index::publish(conn, &pending)
        .await
        .unwrap();
    assert_eq!(replay["published"], false);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM history_fts")
        .fetch_one(&readers)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}
