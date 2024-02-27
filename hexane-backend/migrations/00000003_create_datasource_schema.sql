CREATE SCHEMA datasource;

CREATE TABLE datasource.file(
    id       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id  UUID NOT NULL REFERENCES users.account ON DELETE CASCADE,

    created   TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    deleted   TIMESTAMP WITH TIME ZONE,
    processed TIMESTAMP WITH TIME ZONE,

    name     TEXT   NOT NULL,
    hash     TEXT   NOT NULL,
    path     TEXT   NOT NULL UNIQUE,
    size     BIGINT NOT NULL CHECK ( size > 0 ),
    type     TEXT   NOT NULL CHECK ( LENGTH(type) < 128 ),
    category TEXT   NOT NULL CHECK ( LENGTH(category) < 128 ),

    CONSTRAINT datasource_file_name_length_check
        CHECK ( 0 < LENGTH(name) AND LENGTH(name) < 128 ),

    CONSTRAINT datasource_file_hash_unique
        UNIQUE ( user_id, hash )
);

CREATE TABLE datasource.embedding(
    id      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_id UUID NOT NULL REFERENCES datasource.file ON DELETE CASCADE,

    created TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),

    token  INTEGER CHECK (token IS NULL OR token > 0),
    text   TEXT NOT NULL,
    embedding VECTOR NOT NULL
);

/* capture_func executes a NOTIFY. it is used to listen on new row insertions. */
CREATE FUNCTION capture_datasource_func()
RETURNS trigger AS
$$
DECLARE
  v_txt TEXT;
BEGIN
  v_txt := format('new %s: %s', TG_OP, NEW.id);
  /* RAISE NOTICE '%', v_txt; */
  EXECUTE FORMAT('NOTIFY datasource_insert, ''%s''', v_txt);
  RETURN NEW;
END;
$$ LANGUAGE 'plpgsql';

CREATE CONSTRAINT TRIGGER datasource_file_new_row_trigger
    AFTER INSERT ON datasource.file
    DEFERRABLE
    INITIALLY DEFERRED
    FOR EACH ROW
    EXECUTE PROCEDURE capture_datasource_func();
