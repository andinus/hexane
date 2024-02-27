UPDATE users.account SET credit = credit - $2
  WHERE id = $1;
