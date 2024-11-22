CREATE SCHEMA demo;

CREATE TABLE demo.notification (
    experiment_id TEXT,
    measurement_id TEXT, 
    group_id TEXT,
    latency DOUBLE PRECISION,
    PRIMARY KEY(experiment_id, measurement_id, group_id)
);

CREATE TABLE demo.notification_ground_truth (
    experiment_id TEXT,
    measurement_id TEXT, 
    insert_timestamp TIMESTAMP DEFAULT now(),
    PRIMARY KEY(experiment_id, measurement_id)
);

CREATE USER grafanareader WITH PASSWORD '***';

GRANT USAGE ON SCHEMA demo TO grafanareader;

GRANT SELECT
ON ALL TABLES IN SCHEMA demo
TO grafanareader;
