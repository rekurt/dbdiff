-- Current schema: orders still store a free-form payment date.
CREATE TABLE users (
    id INTEGER NOT NULL,
    email TEXT NOT NULL
);

CREATE TABLE orders (
    id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    total NUMERIC(10,2) NOT NULL DEFAULT 0,
    payment_date TEXT
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
