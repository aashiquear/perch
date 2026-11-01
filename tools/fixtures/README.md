# IMU fixtures

JSON lines, one sample per line: `{"t_us":<u64>,"accel":[x,y,z],"gyro":[x,y,z],"mag":[x,y,z]?}`
(m/s², rad/s, µT, RUB frame). Record with `perch record <file> --seconds N`
(needs `--features rokid`). `synthetic-yaw.jsonl` is generated and used by
docs/examples; real recordings from the Max go here once M1 lands.
