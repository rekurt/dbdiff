-- Schema A: current state (e.g. production)

CREATE TABLE users (
    id serial NOT NULL,
    email varchar(255) NOT NULL,
    payment_date varchar(32),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE orders (
    id serial NOT NULL,
    user_id integer NOT NULL,
    total numeric(10,2) NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
