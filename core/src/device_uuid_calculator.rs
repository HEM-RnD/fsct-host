use uuid::Uuid;

const ROOT_UUID_STR: &str = "0e042ba4-82f1-4531-bd35-b455efebc627";

pub fn calculate_uuid(vid: u16, pid: u16, sn: &str) -> Uuid {
    let hem_root_uuid = Uuid::parse_str(ROOT_UUID_STR).unwrap();
    let vendor_uuid = Uuid::new_v5(&hem_root_uuid, format!("{:04x}", vid).as_bytes());
    let product_uuid = Uuid::new_v5(&vendor_uuid, format!("{:04x}", pid).as_bytes());
    let sn_uuid = Uuid::new_v5(&product_uuid, sn.as_bytes());
    sn_uuid
}

#[cfg(test)]
mod tests {
    use super::calculate_uuid;

    const VID: u16 = 65535;
    const PID: u16 = 32768;
    const SN: &str = "1234abcd0000";

    #[test]
    fn calculate_uuid_executed_two_times_with_the_same_arguments_should_return_twice_the_same_uuid() {
        let uuid_1 = calculate_uuid(VID, PID, SN);
        let uuid_2 = calculate_uuid(VID, PID, SN);

        assert_eq!(uuid_1, uuid_2);
    }

    #[test]
    fn calculate_uuid_executed_with_only_one_changed_argument_from_argument_list_should_return_different_uuid() {
        let vid_mod = 50111_u16;
        let pid_mod = 10222_u16;
        let sn_mod = "9876wxyz9999";

        let uuid_reference = calculate_uuid(VID, PID, SN);

        let uuid_vid_mod = calculate_uuid(vid_mod, PID, SN);
        assert_ne!(uuid_reference, uuid_vid_mod);

        let uuid_pid_mod = calculate_uuid(VID, pid_mod, SN);
        assert_ne!(uuid_reference, uuid_pid_mod);

        let uuid_sn_mod = calculate_uuid(VID, PID, sn_mod);
        assert_ne!(uuid_reference, uuid_sn_mod);
    }
}