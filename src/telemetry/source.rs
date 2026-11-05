//! Stable legacy stream identity and resumable import through the existing store owner.
use rusqlite::{Connection,TransactionBehavior,params};
use coreq::telemetry::{Event,Store};
use ring::rand::{SecureRandom,SystemRandom};

type Res<T> = Result<T,String>;
const SOURCE: &str="taskq.legacy";
const INSERT: &str="CREATE TRIGGER taskq_source_insert AFTER INSERT ON records BEGIN
 UPDATE telemetry_source SET epoch=epoch+(new.seq<=high_seq),high_seq=new.seq WHERE id=1;
 UPDATE records SET telemetry_epoch=(SELECT epoch FROM telemetry_source WHERE id=1) WHERE seq=new.seq;
 END";
const UPDATE: &str="CREATE TRIGGER taskq_source_update AFTER UPDATE OF seq,retained_ms,line ON records
 WHEN old.seq!=new.seq OR old.retained_ms!=new.retained_ms OR old.line!=new.line BEGIN
 UPDATE telemetry_source SET invalid=1 WHERE id=1; END";
const DELETE: &str="CREATE TRIGGER taskq_source_delete AFTER DELETE ON records BEGIN
 UPDATE telemetry_source SET removed_seq=CASE WHEN old.telemetry_epoch>removed_epoch OR (old.telemetry_epoch=removed_epoch AND old.seq>removed_seq) THEN old.seq ELSE removed_seq END,
 removed_epoch=max(removed_epoch,old.telemetry_epoch) WHERE id=1; END";
fn error(e: impl std::fmt::Display) -> String {format!("legacy source: {e}")}
fn namespace(text: &str) -> bool {text.len()==32&&text.bytes().all(|v|v.is_ascii_digit()||(b'a'..=b'f').contains(&v))}

// Caller owns taskq-profile.lock; install changes source metadata only, in one FULL transaction.
fn install_identity(db: &mut Connection) -> Res<()> {
    if db.pragma_query_value(None,"application_id",|r|r.get::<_,i64>(0)).map_err(error)?!=super::APP
        ||db.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).map_err(error)?!=1 {return Err("legacy source: unsupported archive; preserved".into());}
    let check:String=db.query_row("PRAGMA quick_check",[],|r|r.get(0)).map_err(error)?;
    if check!="ok" {return Err("legacy source: corrupt archive; preserved".into());}
    let page:i64=db.pragma_query_value(None,"page_size",|r|r.get(0)).map_err(error)?;
    let pages:i64=db.pragma_query_value(None,"page_count",|r|r.get(0)).map_err(error)?;
    if page<1024||pages>super::LEGACY_DB_MAX/page {return Err("legacy source: oversized archive; preserved".into());}
    if installed(db)? {return Ok(());}
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY; PRAGMA cache_spill=OFF; PRAGMA wal_autocheckpoint=0;").map_err(error)?;
    super::checkpoint(db)?;
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error)?;
    {
        let mut id=[0;16];SystemRandom::new().fill(&mut id).map_err(|_|"legacy source: random identity unavailable")?;
        let id=id.iter().map(|byte|format!("{byte:02x}")).collect::<String>();
        tx.execute_batch("ALTER TABLE records ADD COLUMN telemetry_epoch INTEGER NOT NULL DEFAULT 0;
          CREATE TABLE telemetry_source(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,namespace TEXT NOT NULL,epoch INTEGER NOT NULL,high_seq INTEGER NOT NULL,invalid INTEGER NOT NULL,removed_epoch INTEGER NOT NULL,removed_seq INTEGER NOT NULL);").map_err(error)?;
        tx.execute("INSERT INTO telemetry_source SELECT 1,1,?1,0,coalesce(max(seq),0),0,-1,0 FROM records",[id]).map_err(error)?;
        for sql in [INSERT,UPDATE,DELETE] {tx.execute_batch(sql).map_err(error)?;}
    }
    installed(&tx)?;
    #[cfg(test)]
    if std::env::var("TASKQ_SOURCE_IDENTITY_PHASE").is_ok_and(|phase|phase=="open") {
        std::fs::write(std::env::var_os("TASKQ_SOURCE_IDENTITY_MARKER").ok_or("missing identity marker")?,"transaction-open").map_err(error)?;
        loop {std::thread::park();}
    }
    tx.commit().map_err(error)
}

fn installed(db:&Connection)->Res<bool> {
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='telemetry_source')",[],|r|r.get(0)).map_err(error)?;
    let epoch:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('records') WHERE name='telemetry_epoch')",[],|r|r.get(0)).map_err(error)?;
    let triggers:i64=db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('taskq_source_insert','taskq_source_update','taskq_source_delete')",[],|r|r.get(0)).map_err(error)?;
    if !exists&&!epoch&&triggers==0 {return Ok(false);}
    if !exists||!epoch||triggers!=3 {return Err("legacy source: incomplete identity extension; preserved".into());}
    for (name,sql) in [("taskq_source_insert",INSERT),("taskq_source_update",UPDATE),("taskq_source_delete",DELETE)] {
        let actual:String=db.query_row("SELECT sql FROM sqlite_master WHERE name=?1",[name],|r|r.get(0)).map_err(error)?;
        if actual!=sql {return Err("legacy source: unsupported identity trigger; preserved".into());}
    }
    state(db)?;
    Ok(true)
}

struct State {namespace:String,epoch:i64,high:i64,removed:(i64,i64)}
fn state(db: &Connection) -> Res<State> {
    let count:i64=db.query_row("SELECT count(*) FROM telemetry_source",[],|r|r.get(0)).map_err(error)?;
    if count!=1 {return Err("legacy source: invalid identity cardinality; preserved".into());}
    let (version,id,epoch,high,invalid,removed,seq):(i64,String,i64,i64,i64,i64,i64)=db.query_row("SELECT version,namespace,epoch,high_seq,invalid,removed_epoch,removed_seq FROM telemetry_source WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(error)?;
    if version!=1||!namespace(&id)||epoch<0||high<0||invalid!=0||removed< -1||seq<0||removed>epoch {return Err("legacy source: invalid or mutated source identity; preserved".into());}
    Ok(State{namespace:id,epoch,high,removed:(removed,seq)})
}
fn position(cursor: Option<&str>, identity: &str) -> Res<(i64,i64)> {
    let Some(cursor)=cursor else {return Ok((-1,0));};let words=cursor.split('.').collect::<Vec<_>>();
    if words.len()!=4||words[0]!="v1"||words[1]!=identity {return Err("legacy source: replaced archive identity; resume refused".into());}
    let epoch=words[2].parse::<i64>().map_err(error)?;let seq=words[3].parse::<i64>().map_err(error)?;
    if epoch<0||seq<=0||cursor!=format!("v1.{identity}.{epoch}.{seq}") {return Err("legacy source: invalid progress; preserved".into());}Ok((epoch,seq))
}

pub(super) fn preserved(install:&std::path::Path)->Res<bool> {
    let path=install.join("telemetry.log/telemetry.sqlite3");
    if !path.try_exists().map_err(error)? {return Ok(false);}
    let db=Connection::open_with_flags(&path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(error)?;
    installed(&db)
}

/// Prepare while the existing source owner is quiesced; retain the archive for rollback.
pub(super) fn prepare(install:&std::path::Path)->Res<Connection> {
    let _lock=crate::profile::log_lock(&install.join("taskq.log")).map_err(error)?;
    let path=install.join("telemetry.log/telemetry.sqlite3");
    if path.try_exists().map_err(error)? {
        let db=Connection::open_with_flags(&path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(error)?;
        if installed(&db)? {
            let (log,old)=super::legacy_scan(&db,install)?;
            if log.offset(old.as_ref()).is_ok_and(|after|!log.pending(after)) {
                let mut db=db;install_identity(&mut db)?;return Ok(db);
            }
        }
    }
    let mut db=super::open(install)?;
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(error)?.as_millis().min(u64::MAX as u128) as u64;
    super::import(&mut db,install,now)?;install_identity(&mut db)?;super::checkpoint(&db)?;
    drop(db);
    Connection::open_with_flags(&path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(error)
}

pub(super) fn pending(db:&Connection,cursor:Option<&str>)->Res<Option<(i64,i64)>> {
    if !installed(db)? {
        if cursor.is_some() {return Err("legacy source: imported archive identity missing; coverage unknown".into());}
        return Ok(None);
    }
    let source=state(db)?;let after=position(cursor,&source.namespace)?;
    if after.0>source.epoch||(after.0==source.epoch&&after.1>source.high) {return Err("legacy source: source progress moved backwards; resume refused".into());}
    if source.removed>after {return Err("legacy source: unimported records pruned; coverage unknown".into());}
    Ok(Some(after))
}

fn drained(db:&Connection,install_root:&std::path::Path)->Res<()> {
    let (log,old)=super::legacy_scan(db,install_root)?;
    if log.pending(log.offset(old.as_ref())?) {return Err("legacy taskq.log has undrained telemetry; source import refused".into());}
    Ok(())
}

pub(super) fn import(db: &mut Connection, target: &Store, install_root: &std::path::Path) -> Res<u64> {
    {
        let _lock=crate::profile::log_lock(&install_root.join("taskq.log")).map_err(error)?;
        drained(db,install_root)?;install_identity(db)?;
    }
    let mut imported=0;
    loop {
        let cursor=target.import_cursor(SOURCE)?;
        let lock=crate::profile::read_log_lock(install_root).map_err(error)?;
        let tx=db.transaction().map_err(error)?;drained(&tx,install_root)?;
        let after=pending(&tx,cursor.as_deref())?.ok_or("legacy source: identity missing")?;let source=state(&tx)?;
        let mut read=tx.prepare("SELECT telemetry_epoch,seq,retained_ms,line FROM records WHERE telemetry_epoch>?1 OR (telemetry_epoch=?1 AND seq>?2) ORDER BY telemetry_epoch,seq LIMIT 1024").map_err(error)?;
        let rows=read.query_map(params![after.0,after.1],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?))).map_err(error)?.collect::<Result<Vec<_>,_>>().map_err(error)?;drop(read);
        if rows.is_empty() {return Ok(imported);}
        let mut events=Vec::with_capacity(rows.len());let mut clocks=Vec::with_capacity(rows.len());let mut last=None;let mut processed=0;
        for (epoch,seq,received,line) in &rows {
            if *epoch<0||*epoch>source.epoch||(*epoch==source.epoch&&*seq>source.high)||*seq<=0||*received<0 {return Err("legacy source: invalid record position or clock".into());}
            if line.len()>super::LINE_MAX {return Err("legacy source: record exceeds 64 KiB; preserved".into());}
            let parts=Event::split_identified(&format!("taskq.legacy.{}",source.namespace),&format!("{epoch}.{seq}"),super::semantic::observation(line)?)?;
            if events.len()+parts.len()>1024 {break;}
            clocks.extend(std::iter::repeat_n(*received,parts.len()));events.extend(parts);last=Some((*epoch,*seq));processed+=1;
        }
        let last=last.ok_or("legacy source: source row exceeds import batch")?;let next=format!("v1.{}.{}.{}",source.namespace,last.0,last.1);tx.commit().map_err(error)?;drop(lock);
        target.import_history(SOURCE,cursor.as_deref(),&next,&events,&clocks)?;imported+=processed;
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
