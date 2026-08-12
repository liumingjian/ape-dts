use test_db_1

-- insert docs with nested sub documents and arrays
db.tb_1.insertOne({ "_id": "1", "name": "a", "profile": { "city": "sh", "tags": { "x": 1, "y": 2 } }, "arr": [ { "k": 1 }, { "k": 2 }, 3 ] });
db.tb_1.insertOne({ "_id": "2", "name": "b", "profile": { "city": "bj" }, "arr": [ 1, 2 ] });
db.tb_1.insertOne({ "_id": "3", "name": "c", "profile": { "city": "gz" }, "arr": [ 1 ] });

-- nested $set, generates a $v:2 diff with a sub document diff
db.tb_1.updateOne({ "_id": "1" }, { "$set": { "profile.city": "hz" } });

-- deeper nested $set
db.tb_1.updateOne({ "_id": "1" }, { "$set": { "profile.tags.x": 100 } });

-- nested $unset
db.tb_1.updateOne({ "_id": "1" }, { "$unset": { "profile.tags.y": "" } });

-- array element replaced, generates an array diff
db.tb_1.updateOne({ "_id": "1" }, { "$set": { "arr.2": 30 } });

-- field of a sub document inside an array element
db.tb_1.updateOne({ "_id": "1" }, { "$set": { "arr.0.k": 10 } });

-- $set and $unset in one update, both must survive the diff
db.tb_1.updateOne({ "_id": "1" }, { "$set": { "profile.zip": "310000", "name": "a_1" }, "$unset": { "profile.tags.x": "" } });

-- rename a nested field, the diff carries an insert and a delete in one entry
db.tb_1.updateOne({ "_id": "1" }, { "$rename": { "profile.city": "profile.town" } });

-- array append
db.tb_1.updateOne({ "_id": "2" }, { "$push": { "arr": 3 } });

-- array shrink from the end, the diff carries a resize which replays as $push/$slice
db.tb_1.updateOne({ "_id": "2" }, { "$pop": { "arr": 1 } });

-- new sub document field
db.tb_1.updateOne({ "_id": "2" }, { "$set": { "profile.zip": "100000" } });

-- replacement style update, o of the op_log is the whole new doc
db.tb_1.replaceOne({ "_id": "3" }, { "name": "c_replaced", "profile": { "city": "sz" } });

-- update many with a nested field
db.tb_1.updateMany({ "name": { "$exists": true } }, { "$set": { "profile.synced": true } });
