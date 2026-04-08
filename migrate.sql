BEGIN;

CREATE SEQUENCE audit_log_id_seq AS bigint START 1 INCREMENT 1 MINVALUE 1 MAXVALUE 9223372036854775807;

CREATE TABLE audit_log (
    action varchar(100) NOT NULL,
    actor_id varchar(255) NOT NULL DEFAULT '',
    actor_type varchar(20) NOT NULL DEFAULT 'service',
    balance_after numeric,
    balance_before numeric,
    created_at timestamptz NOT NULL DEFAULT now(),
    details jsonb NOT NULL DEFAULT '{}',
    entity_id varchar(255) NOT NULL,
    entity_type varchar(50) NOT NULL,
    id bigint NOT NULL DEFAULT nextval('audit_log_id_seq')
);

CREATE INDEX idx_audit_log_action ON audit_log(action);

CREATE INDEX idx_audit_log_actor ON audit_log(actor_id, actor_type);

CREATE INDEX idx_audit_log_created_at ON audit_log(created_at);

CREATE INDEX idx_audit_log_entity ON audit_log(entity_type, entity_id);

ALTER TABLE audit_log ADD CONSTRAINT audit_log_pkey PRIMARY KEY (id);

ALTER TABLE accounts ADD COLUMN freeze_reason varchar(255) DEFAULT '';

-- !!  On PostgreSQL < 11, adding NOT NULL column 'held_balance' with DEFAULT will rewrite the entire table and acquire AccessExclusiveLock.
ALTER TABLE accounts ADD COLUMN held_balance numeric NOT NULL DEFAULT 0;

ALTER TABLE operations ADD COLUMN amount_usd numeric;

ALTER TABLE transactions ADD COLUMN amount_usd numeric;

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_accounts_owner_asset_active ON accounts(owner, asset_symbol);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_accounts_type_asset_active ON accounts(type, asset_symbol);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE UNIQUE INDEX idx_accounts_unique_owner_asset_type_active ON accounts(owner, asset_symbol, type);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_operations_account_to_created ON operations(account_to_id, created_at);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_transactions_created_at ON transactions(created_at);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_transactions_status ON transactions(status);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_transactions_type_status_created ON transactions(type, status, created_at);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE INDEX idx_transactions_user_id_created_at ON transactions(user_id, created_at);

-- !!  Consider using CREATE INDEX CONCURRENTLY to avoid locking the table.
CREATE UNIQUE INDEX uq_transactions_external_id ON transactions(external_id);

ALTER TABLE accounts ADD CONSTRAINT chk_accounts_balance_non_negative CHECK (((balance >= (0)::numeric) OR ((type)::text = ANY (ARRAY[('system'::character varying)::text, ('external'::character varying)::text]))));

ALTER TABLE accounts ADD CONSTRAINT chk_accounts_held_balance_non_negative CHECK ((held_balance >= (0)::numeric));

ALTER TABLE accounts ADD CONSTRAINT chk_accounts_held_le_balance CHECK (((held_balance <= balance) OR ((type)::text = ANY (ARRAY[('system'::character varying)::text, ('external'::character varying)::text]))));

ALTER TABLE operations ADD CONSTRAINT chk_operations_amount_positive CHECK ((amount > (0)::numeric));

COMMIT;