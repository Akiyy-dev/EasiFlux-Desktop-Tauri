use easiflux_desktop_lib::news_provision::{
    parse_os_arguments, run, EXIT_KEYRING_FAILURE, EXIT_USAGE_OR_INVALID_TOKEN,
};
use easiflux_desktop_lib::KeyringNewsTokenStore;
use std::io::{self, Cursor, IsTerminal};
use zeroize::Zeroizing;

fn main() {
    let arguments = match parse_os_arguments(std::env::args_os().skip(1)) {
        Ok(arguments) => arguments,
        Err(_) => {
            eprintln!("invalid usage or token");
            std::process::exit(EXIT_USAGE_OR_INVALID_TOKEN);
        }
    };
    let store = match KeyringNewsTokenStore::new() {
        Ok(store) => store,
        Err(_) => {
            eprintln!("keyring failure");
            std::process::exit(EXIT_KEYRING_FAILURE);
        }
    };

    let code = if arguments.as_slice() == ["set"] && io::stdin().is_terminal() {
        match rpassword::read_password() {
            Ok(password) => {
                let password = Zeroizing::new(password);
                run(
                    &arguments,
                    &mut Cursor::new(password.as_bytes()),
                    &store,
                    &mut io::stdout(),
                    &mut io::stderr(),
                )
            }
            Err(_) => {
                eprintln!("invalid usage or token");
                EXIT_USAGE_OR_INVALID_TOKEN
            }
        }
    } else {
        run(
            &arguments,
            &mut io::stdin().lock(),
            &store,
            &mut io::stdout(),
            &mut io::stderr(),
        )
    };

    std::process::exit(code);
}
