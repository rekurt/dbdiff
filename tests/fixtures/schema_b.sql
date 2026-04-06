-- Schema B: desired state (e.g. staging)

CREATE TABLE users (
    id serial NOT NULL,
    email varchar(255) NOT NULL,
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE orders (
    id serial NOT NULL,
    user_id integer NOT NULL,
    total numeric(10,2) NOT NULL DEFAULT 0,
    paid_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
CREATE INDEX idx_orders_paid_at ON orders(paid_at);

CREATE TABLE audit_log (
    id serial NOT NULL,
    action text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
