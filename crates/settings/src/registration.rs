//! Runtime support for wiring settings into the [`SettingsManager`].
//!
//! The registration body is generic over the group model and value type so
//! settings that share those types share one compiled instantiation, while
//! [`SettingCallbacks`] preserves setting-specific method resolution.

use anyhow::{Result, anyhow};
use serde::de::DeserializeOwned;
use settings_value::SettingsValue;
use torchui_core::{
    AddSingletonModel, Entity, GetSingletonModelHandle, ModelContext, ModelHandle, SingletonEntity,
    UpdateModel,
};

use crate::manager::SettingsManager;
use crate::{Setting, SupportedPlatforms};

/// Parses a serialized setting value. Tries the settings-file representation
/// first (which handles snake_case enums and other file forms), then falls
/// back to plain serde for cloud sync values.
fn parse_value<V: SettingsValue + DeserializeOwned>(serialized: &str) -> Option<V> {
    serde_json::from_str::<serde_json::Value>(serialized)
        .ok()
        .and_then(|json_val| V::from_file_value(&json_val))
        .or_else(|| serde_json::from_str(serialized).ok())
}

/// Strict variant of [`parse_value`] for the equality callback. It propagates
/// JSON syntax errors, so validation can detect malformed stored values.
fn parse_value_strict<V: SettingsValue + DeserializeOwned>(
    serialized: &str,
    storage_key: &'static str,
) -> Result<V> {
    let json_val = serde_json::from_str::<serde_json::Value>(serialized)?;
    V::from_file_value(&json_val)
        .or_else(|| serde_json::from_str(serialized).ok())
        .ok_or_else(|| anyhow!("Failed to parse value for {}", storage_key))
}

/// Compares two serialized values of the given setting for semantic equality.
fn equals_serialized<S: Setting>(left: &str, right: &str) -> Result<bool> {
    let left_setting = S::new(Some(parse_value_strict::<S::Value>(
        left,
        S::storage_key(),
    )?));
    let right_setting = S::new(Some(parse_value_strict::<S::Value>(
        right,
        S::storage_key(),
    )?));
    Ok(left_setting.value() == right_setting.value())
}

/// Typed operations on one setting within its group model.
///
/// The `register_settings_events!` macro constructs this struct at its
/// expansion site, with the concrete group and setting types. This keeps
/// method resolution local to the concrete setting, so an inherent method
/// (for example `current_value_is_syncable`) can shadow the [`Setting`] trait
/// default.
pub struct SettingCallbacks<G: Entity, V> {
    /// Applies an updated value with `set_value`.
    pub apply_set: fn(&mut G, V, &mut ModelContext<G>) -> Result<()>,
    /// Loads a value into memory with `load_value`; the second argument is
    /// whether the value was explicitly set.
    pub apply_load: fn(&mut G, V, bool, &mut ModelContext<G>) -> Result<()>,
}

impl<G: Entity, V> Clone for SettingCallbacks<G, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<G: Entity, V> Copy for SettingCallbacks<G, V> {}

/// The per-setting values that [`register_setting_events`] gathers from the
/// [`Setting`] trait before it hands off to the shared registration body.
struct SettingMetadata {
    storage_key: &'static str,
    supported_platforms: SupportedPlatforms,
    serialized_default_value: String,
    file_serialized_default_value: String,
    hierarchy: Option<&'static str>,
    toml_key: &'static str,
    max_table_depth: Option<u32>,
    is_private: bool,
}

/// Registers listeners for settings events that get piped through the
/// [`SettingsManager`]. These events allow anyone to listen to settings
/// changes based on storage key rather than individual settings models.
///
/// This function gathers the per-setting metadata and then delegates to a
/// body that is generic over only the group and value types, which keeps the
/// per-setting compiled code small.
pub fn register_setting_events<S, C>(
    settings_group: ModelHandle<S::Group>,
    callbacks: SettingCallbacks<S::Group, S::Value>,
    ctx: &mut C,
) where
    S: Setting + 'static,
    <S::Group as Entity>::Event: 'static,
    C: GetSingletonModelHandle + AddSingletonModel + UpdateModel,
{
    let serialized_default_value =
        serde_json::to_string(&S::default_value()).expect("default should serialize");
    let file_serialized_default_value = {
        let file_value = S::default_value().to_file_value();
        serde_json::to_string(&file_value).expect("default file value should serialize")
    };
    register_setting_events_impl(
        settings_group,
        SettingMetadata {
            storage_key: S::storage_key(),
            supported_platforms: S::supported_platforms(),
            serialized_default_value,
            file_serialized_default_value,
            hierarchy: S::hierarchy(),
            toml_key: S::toml_key(),
            max_table_depth: S::max_table_depth(),
            is_private: S::is_private(),
        },
        callbacks,
        equals_serialized::<S>,
        ctx,
    );
}

/// The shared registration body. Generic only over the group model, the
/// value type, and the context, so settings that share those types share one
/// compiled instantiation.
fn register_setting_events_impl<G, V, C>(
    settings_group: ModelHandle<G>,
    metadata: SettingMetadata,
    callbacks: SettingCallbacks<G, V>,
    equals: fn(&str, &str) -> Result<bool>,
    ctx: &mut C,
) where
    G: Entity,
    G::Event: 'static,
    V: SettingsValue + DeserializeOwned + 'static,
    C: GetSingletonModelHandle + AddSingletonModel + UpdateModel,
{
    SettingsManager::handle(ctx).update(ctx, |manager, _| {
        let storage_key = metadata.storage_key;
        // Register callbacks for updating individual settings model by storage key.
        let settings_group_update_clone = settings_group.clone();
        let settings_group_load_clone = settings_group.clone();
        manager.register_setting(
            metadata.storage_key,
            metadata.supported_platforms,
            metadata.serialized_default_value,
            metadata.file_serialized_default_value,
            metadata.hierarchy,
            metadata.toml_key,
            metadata.max_table_depth,
            metadata.is_private,
            move |value, ctx| {
                let Some(value) = parse_value::<V>(&value) else {
                    return Err(anyhow!(
                        "Failed to parse updated value for setting {}: Not updating",
                        storage_key
                    ));
                };
                settings_group_update_clone.update(ctx, |settings_group, ctx| {
                    (callbacks.apply_set)(settings_group, value, ctx)
                })
            },
            move |value, explicitly_set, ctx| {
                let Some(value) = parse_value::<V>(&value) else {
                    return Err(anyhow!(
                        "Failed to parse loaded value for setting {}: Not loading",
                        storage_key
                    ));
                };
                settings_group_load_clone.update(ctx, |settings_group, ctx| {
                    (callbacks.apply_load)(settings_group, value, explicitly_set, ctx)
                })
            },
            equals,
        );
    });
}
