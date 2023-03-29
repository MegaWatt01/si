CREATE TABLE fixes
(
    pk                          ident primary key                 default ident_create_v1(),
    id                          ident                    not null default ident_create_v1(),
    tenancy_workspace_pk        ident,
    visibility_change_set_pk    ident                    NOT NULL DEFAULT ident_nil_v1(),
    visibility_deleted_at       timestamp with time zone,
    created_at                  timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    updated_at                  timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    attribute_value_id          ident                    NOT NULL,
    component_id                ident                    NOT NULL,
    action                      text                     NOT NULL,
    workflow_runner_id          ident,
    started_at                  text,
    finished_at                 text,
    completion_status           text,
    completion_message          text
);

-- TODO(nick): create a better unique index.
-- CREATE UNIQUE INDEX unique_fixes
--     ON fixes (attribute_value_id,
--               component_id,
--               tenancy_workspace_pk,
--               visibility_change_set_pk);

SELECT standard_model_table_constraints_v1('fixes');
SELECT belongs_to_table_create_v1(
               'fix_belongs_to_fix_batch',
               'fixes',
               'fix_batches'
           );
INSERT INTO standard_models (table_name, table_type, history_event_label_base, history_event_message_name)
VALUES ('fixes', 'model', 'fix', 'Fix'),
       ('fix_belongs_to_fix_batch', 'belongs_to', 'fix_batch.fix',
        'Fix Batch <> Fix');

CREATE OR REPLACE FUNCTION fix_create_v1(
    this_tenancy jsonb,
    this_visibility jsonb,
    this_attribute_value_id ident,
    this_component_id ident,
    this_action text,
    OUT object json) AS
$$
DECLARE
    this_tenancy_record    tenancy_record_v1;
    this_visibility_record visibility_record_v1;
    this_new_row           fixes%ROWTYPE;
BEGIN
    this_tenancy_record := tenancy_json_to_columns_v1(this_tenancy);
    this_visibility_record := visibility_json_to_columns_v1(this_visibility);

    INSERT INTO fixes (tenancy_workspace_pk, visibility_change_set_pk,
                       attribute_value_id, component_id, action)
    VALUES (this_tenancy_record.tenancy_workspace_pk,
            this_visibility_record.visibility_change_set_pk,
            this_attribute_value_id, this_component_id, this_action)
    RETURNING * INTO this_new_row;

    object := row_to_json(this_new_row);
END
$$ LANGUAGE PLPGSQL VOLATILE;
