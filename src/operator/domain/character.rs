use super::*;

pub(super) async fn start(db: &mut SqliteConnection, service: &Service) -> Result<()> {
    if get(db, "character-profile").await?.is_none() {
        let id = crate::identity::new_id(db).await?;
        put(
            db,
            "character-profile",
            &json!({"id":id,"slug":id,"display_name":service.name,
            "bio":"","accent":{"light":"#3A6BD6","dark":"#7FA4FF"},"revision":1,
            "avatar_revision":0,"avatar":null,"default_avatar":null}),
        )
        .await?;
    } else if get(db, "character-name-managed").await?.is_none() {
        let mut value = read(db).await?;
        value["display_name"] = json!(service.name);
        put(db, "character-profile", &value).await?;
    }
    Ok(())
}
pub(super) async fn read(db: &mut SqliteConnection) -> Result<Value> {
    get(db, "character-profile")
        .await?
        .ok_or_else(|| "character_unavailable".into())
}
pub(super) async fn identity(db: &mut SqliteConnection, service: &Service) -> Result<Value> {
    let profile = read(db).await?;
    let name = profile["display_name"]
        .as_str()
        .ok_or("invalid_character")?;
    let mut aliases = Vec::new();
    for alias in std::iter::once(name).chain(service.aliases.iter().map(String::as_str)) {
        if !alias.trim().is_empty() && !aliases.contains(&alias) {
            aliases.push(alias);
        }
    }
    Ok(
        json!({"id":profile["id"],"display_name":name,"aliases":aliases,"revision":profile["revision"]}),
    )
}
pub(super) async fn update(
    db: &mut SqliteConnection,
    actor: &Actor,
    body: &Value,
) -> Result<Value> {
    validate("CharacterUpdateBody", body)?;
    let method = "character.update";
    if let Some(old) = previous(db, actor, method, body).await? {
        return Ok(old["result"].clone());
    }
    let object = body.as_object().ok_or("validation_error")?;
    if object.keys().any(|k| {
        !matches!(
            k.as_str(),
            "request_id" | "expected_revision" | "display_name" | "bio" | "slug" | "accent"
        )
    }) {
        return Err("validation_error".into());
    }
    let mut profile = read(db).await?;
    if body["expected_revision"] != profile["revision"] {
        return Err("version_conflict".into());
    }
    for name in ["display_name", "bio", "slug", "accent"] {
        if let Some(value) = body.get(name) {
            profile[name] = value.clone()
        }
    }
    profile["revision"] = json!(
        profile["revision"]
            .as_u64()
            .ok_or("invalid_character_revision")?
            .checked_add(1)
            .ok_or("revision_exhausted")?
    );
    validate("CharacterProfile", &profile)?;
    put(db, "character-profile", &profile).await?;
    if body.get("display_name").is_some() {
        put(db, "character-name-managed", &json!(true)).await?;
    }
    record(db, actor, method, body, &profile).await?;
    Ok(profile)
}
