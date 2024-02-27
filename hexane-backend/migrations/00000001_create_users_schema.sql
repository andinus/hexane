CREATE SCHEMA users;

CREATE TABLE users.account(
    id UUID  PRIMARY KEY DEFAULT gen_random_uuid(),
    created TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    deleted TIMESTAMP WITH TIME ZONE,
    verified TIMESTAMP WITH TIME ZONE,

    username TEXT NOT NULL UNIQUE,
    email    TEXT NOT NULL UNIQUE,
    password TEXT NOT NULL,

    CONSTRAINT users_account_username_length_check
        CHECK ( 3 < LENGTH(username) AND LENGTH(username) < 64 ),

    CONSTRAINT users_account_email_length_check
        CHECK ( 0 < LENGTH(email) AND LENGTH(email) < 256 )
);
