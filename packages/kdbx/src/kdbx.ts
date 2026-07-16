class KdbxDatabase {
  static create(name = ''): KdbxDatabase {
    const database = new KdbxDatabase();
    database.meta.databaseName = name;
    return database;
  }
}
