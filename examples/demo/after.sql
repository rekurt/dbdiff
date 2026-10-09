-- Desired schema: typed payment timestamps and soft deletion.
CREATE TABLE users (
    id INTEGER NOT NULL,
    email TEXT NOT NULL,
    deleted_at TIMESTAMP
);

CREATE TABLE orders (
    id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    total NUMERIC(10,2) NOT NULL DEFAULT 0,
    paid_at TIMESTAMP
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
CREATE INDEX idx_orders_paid_at ON orders(paid_at);
