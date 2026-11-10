-- Roles and permissions, the Spatie laravel-permission shape that Filament Shield uses.
-- A permission is a named ability such as `users.viewAny`. The panel registers the
-- permissions of each resource when the app starts, and the `ignite` command grants
-- them to roles and roles to users.
CREATE TABLE IF NOT EXISTS roles (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    UNIQUE KEY roles_name_unique (name)
);

CREATE TABLE IF NOT EXISTS permissions (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    UNIQUE KEY permissions_name_unique (name)
);

CREATE TABLE IF NOT EXISTS role_permission (
    role_id BIGINT NOT NULL,
    permission_id BIGINT NOT NULL,
    PRIMARY KEY (role_id, permission_id),
    CONSTRAINT role_permission_role_fk FOREIGN KEY (role_id) REFERENCES roles (id) ON DELETE CASCADE,
    CONSTRAINT role_permission_permission_fk FOREIGN KEY (permission_id) REFERENCES permissions (id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS role_user (
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    PRIMARY KEY (user_id, role_id),
    CONSTRAINT role_user_user_fk FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    CONSTRAINT role_user_role_fk FOREIGN KEY (role_id) REFERENCES roles (id) ON DELETE CASCADE
);

-- The one row this migration writes. Users who exist before roles are introduced keep
-- full access, so an upgrade does not lock anyone out. Users created later get no roles
-- until `ignite user role add` gives them one.
INSERT INTO roles (name) VALUES ('super-admin');
INSERT INTO role_user (user_id, role_id)
    SELECT users.id, roles.id FROM users CROSS JOIN roles WHERE roles.name = 'super-admin';
