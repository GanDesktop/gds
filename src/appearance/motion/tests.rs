use super::Policy;

#[test]
fn preference_off_on_restores_system_and_late_system_updates_cannot_enable() {
    let mut policy = Policy::default();
    for system in [false, true] {
        policy.system_reduced = system;
        policy.enabled = false;
        assert!(policy.reduced());
        policy.system_reduced = !system;
        assert!(policy.reduced());
        policy.enabled = true;
        assert_eq!(policy.reduced(), !system);
        policy.enabled = false;
        policy.system_reduced = system;
        policy.enabled = true;
        assert_eq!(policy.reduced(), system);
    }
}
