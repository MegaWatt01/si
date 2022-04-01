use crate::schema::builtins::create_prop;
use crate::schema::builtins::kubernetes_metadata::create_metadata_prop;
use crate::schema::builtins::kubernetes_spec::create_spec_prop;
use crate::schema::SchemaResult;
use crate::DalContext;
use crate::{Prop, PropId, PropKind, StandardModel};

#[allow(clippy::too_many_arguments)]
pub async fn create_template_prop(
    ctx: &DalContext<'_, '_>,
    parent_prop_id: Option<PropId>,
) -> SchemaResult<Prop> {
    let template_prop = create_prop(ctx, "template", PropKind::Object, parent_prop_id).await?;

    {
        let _optional_metadata_prop =
            create_metadata_prop(ctx, false, Some(*template_prop.id())).await?;
    }

    {
        let _spec_prop = create_spec_prop(ctx, *template_prop.id()).await?;
    }
    Ok(template_prop)
}
