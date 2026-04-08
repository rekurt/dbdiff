-- Schema C: schema with constraints, views, and more complex structure

CREATE TABLE users (
    id serial NOT NULL,
    email varchar(255) NOT NULL,
    status varchar(20) NOT NULL DEFAULT 'active',
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT unique_email UNIQUE (email),
    CONSTRAINT check_status CHECK (status IN ('active', 'inactive', 'suspended'))
);

CREATE TABLE orders (
    id serial NOT NULL,
    user_id integer NOT NULL,
    total numeric(10,2) NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT fk_orders_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
CREATE INDEX idx_users_email ON users(email);
