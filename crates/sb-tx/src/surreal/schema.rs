#![cfg(feature = "surreal")]

/// Returns the DDL statements required to provision the SurrealDB schema for
/// sb-tx.
pub fn migrations() -> &'static [&'static str] {
    const MIGRATIONS: [&str; 4] = [
        r#"
        DEFINE TABLE tx_outbox SCHEMAFULL;
        DEFINE FIELD tenant ON tx_outbox TYPE string;
        DEFINE FIELD message_id ON tx_outbox TYPE string;
        DEFINE FIELD envelope_id ON tx_outbox TYPE string;
        DEFINE FIELD topic ON tx_outbox TYPE string;
        DEFINE FIELD payload ON tx_outbox TYPE object;
        DEFINE FIELD created_at ON tx_outbox TYPE number;
        DEFINE FIELD not_before ON tx_outbox TYPE number;
        DEFINE FIELD attempts ON tx_outbox TYPE number;
        DEFINE FIELD status ON tx_outbox TYPE string;
        DEFINE FIELD last_error ON tx_outbox TYPE option<string>;
        DEFINE FIELD dispatch_key ON tx_outbox TYPE option<string>;
        DEFINE FIELD lease_until ON tx_outbox TYPE option<number>;
        DEFINE FIELD worker ON tx_outbox TYPE option<string>;
        DEFINE INDEX tx_outbox_message_lookup ON TABLE tx_outbox FIELDS tenant, message_id UNIQUE;
        DEFINE INDEX tx_outbox_ready ON TABLE tx_outbox FIELDS tenant, status, not_before;
        DEFINE INDEX tx_outbox_dispatch_key ON TABLE tx_outbox FIELDS tenant, dispatch_key;
        "#,
        r#"
        DEFINE TABLE tx_idempo SCHEMAFULL;
        DEFINE FIELD tenant ON tx_idempo TYPE string;
        DEFINE FIELD key ON tx_idempo TYPE string;
        DEFINE FIELD hash ON tx_idempo TYPE string;
        DEFINE FIELD status ON tx_idempo TYPE string;
        DEFINE FIELD result_digest ON tx_idempo TYPE option<string>;
        DEFINE FIELD last_error ON tx_idempo TYPE option<string>;
        DEFINE FIELD ttl_ms ON tx_idempo TYPE number;
        DEFINE FIELD created_at ON tx_idempo TYPE number;
        DEFINE FIELD updated_at ON tx_idempo TYPE number;
        DEFINE INDEX tx_idempo_key_lookup ON TABLE tx_idempo FIELDS tenant, key UNIQUE;
        "#,
        r#"
        DEFINE TABLE tx_saga SCHEMAFULL;
        DEFINE FIELD tenant ON tx_saga TYPE string;
        DEFINE FIELD saga_id ON tx_saga TYPE string;
        DEFINE FIELD state ON tx_saga TYPE string;
        DEFINE FIELD def_name ON tx_saga TYPE string;
        DEFINE FIELD steps ON tx_saga TYPE array;
        DEFINE FIELD cursor ON tx_saga TYPE number;
        DEFINE FIELD created_at ON tx_saga TYPE number;
        DEFINE FIELD updated_at ON tx_saga TYPE number;
        DEFINE FIELD timeout_at ON tx_saga TYPE option<number>;
        DEFINE INDEX tx_saga_lookup ON TABLE tx_saga FIELDS tenant, saga_id UNIQUE;
        "#,
        r#"
        DEFINE TABLE tx_dead_letter SCHEMAFULL;
        DEFINE FIELD tenant ON tx_dead_letter TYPE string;
        DEFINE FIELD reference_kind ON tx_dead_letter TYPE string;
        DEFINE FIELD reference_id ON tx_dead_letter TYPE string;
        DEFINE FIELD payload ON tx_dead_letter TYPE object;
        DEFINE FIELD error ON tx_dead_letter TYPE option<string>;
        DEFINE FIELD occurred_at ON tx_dead_letter TYPE number;
        DEFINE INDEX tx_dead_letter_lookup ON TABLE tx_dead_letter FIELDS tenant, reference_kind, reference_id UNIQUE;
        DEFINE INDEX tx_dead_letter_kind ON TABLE tx_dead_letter FIELDS tenant, reference_kind;
        "#,
    ];
    &MIGRATIONS
}
