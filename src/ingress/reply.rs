use crate::Result;
use sqlx::SqliteConnection;

pub(crate) struct HumanReply {
    pub event: i64,
    pub person: String,
    pub observed_generation: Option<i64>,
}

pub(crate) async fn group_human_reply(
    conn: &mut SqliteConnection,
    event: i64,
) -> Result<Option<HumanReply>> {
    let matches: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT reply.id,reply.raw_actor,reply.observed_generation FROM events source
         JOIN entries g ON g.id=source.entry_id AND g.scope='group' AND g.closed_at IS NULL
         JOIN json_each(source.envelope,'$.segments') part ON json_extract(part.value,'$.type')='reply'
         JOIN event_locators l ON l.receiver=source.bot_stage AND l.native_id=json_extract(part.value,'$.native_id')
         JOIN events reply ON reply.id=l.event_id AND reply.entry_id=source.entry_id AND reply.bot_stage=source.bot_stage AND reply.id<source.id
         JOIN assignments author ON author.id=reply.actor_stage AND author.kind='person' AND author.original_person=reply.raw_actor AND author.status='active'
         JOIN assignments bot ON bot.id=reply.bot_stage AND bot.status='active'
         JOIN persons p ON p.id=reply.raw_actor AND p.enabled
         JOIN persons root ON root.id=coalesce(p.current_root,p.id) AND root.enabled
         WHERE source.id=? AND json_extract(reply.envelope,'$.kind')='message'
         AND (SELECT count(*) FROM json_each(source.envelope,'$.segments') p WHERE json_extract(p.value,'$.type')='reply')=1
         AND (SELECT count(*) FROM event_locators candidates JOIN events e ON e.id=candidates.event_id WHERE candidates.receiver=source.bot_stage AND candidates.native_id=l.native_id AND e.entry_id=source.entry_id AND e.id<source.id)=1
         AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source IN(source.id,reply.id)) LIMIT 2",
    )
    .bind(event)
    .fetch_all(conn)
    .await?;
    let mut matches = matches.into_iter();
    let (Some((event, person, observed_generation)), None) = (matches.next(), matches.next())
    else {
        return Ok(None);
    };
    Ok(Some(HumanReply {
        event,
        person,
        observed_generation,
    }))
}
