// Standard Uses

// Crate Uses

// External Uses

const EXPECTED_GENERATED_CODE: &str = r"
trait Registry {
    fn register(message: Credentials) -> RegisterStatus;
    fn my_username() -> String;
    fn tell_back(message: String) -> Result<String, TellBackError>;
}

#[derive(ValidatorStringBounds<[u8; 8]>)]
struct Credentials {

}

struct RegisterStatus {

}

struct TellBackError {

}
";

mod generated {

}


fn main() {
    todo!()
}

