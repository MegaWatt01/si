use dal::DalContext;

use crate::dal::test;
use dal::func::backend::js_qualification::FuncBackendJsQualificationArgs;
use dal::qualification_prototype::QualificationPrototypeContext;
use dal::test_harness::find_or_create_production_system;
use dal::{
    qualification_prototype::UNSET_ID_VALUE, Component, Func, QualificationPrototype, Schema,
    StandardModel,
};

#[test]
async fn new(ctx: &DalContext<'_, '_>) {
    let _ = find_or_create_production_system(ctx).await;

    let name = "docker_image".to_string();
    let schema = Schema::find_by_attr(ctx, "name", &name)
        .await
        .expect("cannot find docker image")
        .pop()
        .expect("no docker image found");
    let (component, _node) = Component::new_for_schema_with_node(ctx, &name, schema.id())
        .await
        .expect("could not create component");

    let func_name = "si:qualificationDockerImageNameInspect".to_string();
    let mut funcs = Func::find_by_attr(ctx, "name", &func_name)
        .await
        .expect("Error fetching builtin function");
    let func = funcs
        .pop()
        .expect("Missing builtin function si:qualificationDockerImageNameInspect");

    let args = FuncBackendJsQualificationArgs {
        component: component
            .veritech_qualification_check_component(ctx, UNSET_ID_VALUE.into())
            .await
            .expect("could not create component qualification view"),
    };

    let mut prototype_context = QualificationPrototypeContext::new();
    prototype_context.set_component_id(*component.id());
    let _prototype = QualificationPrototype::new(
        ctx,
        *func.id(),
        serde_json::to_value(&args).expect("serialization failed"),
        prototype_context,
        "docker image name must match component name",
    )
    .await
    .expect("cannot create new prototype");
}

#[test]
async fn find_for_component(ctx: &DalContext<'_, '_>) {
    // TODO: This test is brittle, because it relies on the behavior of docker_image. I'm okay
    // with that for now, but not for long. If it breaks before we fix it - future person, I'm
    // sorry. ;)

    let name = "docker_image".to_string();
    let schema = Schema::find_by_attr(ctx, "name", &name)
        .await
        .expect("cannot find docker image")
        .pop()
        .expect("no docker image found");
    let default_schema_variant_id = schema
        .default_schema_variant_id()
        .expect("cannot get default schema variant id");

    let (component, _node) = Component::new_for_schema_with_node(ctx, "silverado", schema.id())
        .await
        .expect("cannot create new component");

    let mut found_prototypes = QualificationPrototype::find_for_component(
        ctx,
        *component.id(),
        *schema.id(),
        *default_schema_variant_id,
        UNSET_ID_VALUE.into(),
    )
    .await
    .expect("could not create component qualification view");
    assert_eq!(found_prototypes.len(), 1);
    let _found = found_prototypes
        .pop()
        .expect("found no qualification prototypes");
}
