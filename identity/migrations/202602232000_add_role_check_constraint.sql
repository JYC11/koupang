UPDATE users SET role = 'BUYER' WHERE role NOT IN ('BUYER', 'SELLER', 'ADMIN');
ALTER TABLE users ADD CONSTRAINT chk_user_role CHECK (role IN ('BUYER', 'SELLER', 'ADMIN'));
