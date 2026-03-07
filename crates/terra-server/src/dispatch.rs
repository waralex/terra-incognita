use crate::error::ApiError;
use crate::query::Command;
use crate::state::AppState;

pub fn dispatch(cmd: Command, state: &AppState) -> Result<serde_yaml::Value, ApiError> {
    let registry = state.lock().unwrap();

    match cmd {
        Command::CreateEntityType { slug, description } => {
            let et = registry.create_entity_type(&slug, description.as_deref())?;
            Ok(serde_yaml::to_value(&et).unwrap())
        }
        Command::ListEntityTypes => {
            let types = registry.list_entity_types()?;
            Ok(serde_yaml::to_value(&types).unwrap())
        }
        Command::GetEntityType { slug } => {
            let et = registry.get_entity_type(&slug)?;
            let props = registry.list_properties(&slug)?;

            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("id".into()),
                serde_yaml::to_value(&et.id).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("slug".into()),
                serde_yaml::Value::String(et.slug),
            );
            if let Some(desc) = et.description {
                map.insert(
                    serde_yaml::Value::String("description".into()),
                    serde_yaml::Value::String(desc),
                );
            }
            map.insert(
                serde_yaml::Value::String("created_at".into()),
                serde_yaml::to_value(&et.created_at).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("properties".into()),
                serde_yaml::to_value(&props).unwrap(),
            );
            Ok(serde_yaml::Value::Mapping(map))
        }
        Command::CreateProperty { slug, value_type, description } => {
            let prop = registry.create_property(&slug, value_type, description.as_deref())?;
            Ok(serde_yaml::to_value(&prop).unwrap())
        }
        Command::ListProperties { entity_type: None } => {
            let props = registry.list_all_properties()?;
            Ok(serde_yaml::to_value(&props).unwrap())
        }
        Command::ListProperties {
            entity_type: Some(et),
        } => {
            let props = registry.list_properties(&et)?;
            Ok(serde_yaml::to_value(&props).unwrap())
        }
        Command::AttachProperty { entity_type, slug } => {
            registry.attach_property(&entity_type, &slug)?;
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("status".into()),
                serde_yaml::Value::String("ok".into()),
            );
            Ok(serde_yaml::Value::Mapping(map))
        }
    }
}
