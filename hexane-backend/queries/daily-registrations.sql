SELECT COUNT(*)
FROM users.account
-- accounts that were created within 1 day.
WHERE created > (now() - interval '1 day');
