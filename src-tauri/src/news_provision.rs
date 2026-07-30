use crate::storage::news_token::{MAX_NEWS_TOKEN_BYTES, NEWS_KEYRING_ENTRY, NEWS_KEYRING_SERVICE};
use crate::storage::{NewsApiToken, NewsTokenStore};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use zeroize::Zeroizing;

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_USAGE_OR_INVALID_TOKEN: i32 = 2;
pub const EXIT_KEYRING_FAILURE: i32 = 3;
pub const EXIT_NOT_CONFIGURED: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidProvisionArguments;

pub fn parse_os_arguments<I>(arguments: I) -> Result<Vec<String>, InvalidProvisionArguments>
where
    I: IntoIterator<Item = OsString>,
{
    let arguments = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| InvalidProvisionArguments)
        })
        .collect::<Result<Vec<_>, _>>()?;

    if arguments.iter().all(|argument| argument.is_ascii()) && valid_command(&arguments) {
        Ok(arguments)
    } else {
        Err(InvalidProvisionArguments)
    }
}

fn valid_command(args: &[String]) -> bool {
    matches!(args, [command] if command == "set" || command == "status")
        || matches!(
            args,
            [command, service_flag, service, entry_flag, entry]
                if command == "delete"
                    && service_flag == "--service"
                    && service == NEWS_KEYRING_SERVICE
                    && entry_flag == "--entry"
                    && entry == NEWS_KEYRING_ENTRY
        )
}

pub fn run<R, S, W, E>(
    args: &[String],
    reader: &mut R,
    store: &S,
    stdout: &mut W,
    stderr: &mut E,
) -> i32
where
    R: Read,
    S: NewsTokenStore,
    W: Write,
    E: Write,
{
    if !valid_command(args) {
        return usage_error(stderr);
    }

    match args {
        [command] if command == "set" => run_set(reader, store, stdout, stderr),
        [command] if command == "status" => run_status(store, stdout, stderr),
        [command, service_flag, service, entry_flag, entry]
            if command == "delete"
                && service_flag == "--service"
                && service == NEWS_KEYRING_SERVICE
                && entry_flag == "--entry"
                && entry == NEWS_KEYRING_ENTRY =>
        {
            run_delete(store, stdout, stderr)
        }
        _ => usage_error(stderr),
    }
}

fn run_set<R: Read, S: NewsTokenStore, W: Write, E: Write>(
    reader: &mut R,
    store: &S,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    let input = match read_bounded_input(reader) {
        Ok(input) => input,
        Err(_) => return usage_error(stderr),
    };
    let token = match NewsApiToken::parse(&input) {
        Ok(token) => token,
        Err(_) => return usage_error(stderr),
    };

    match store.set(&token) {
        Ok(()) => {
            let _ = writeln!(stdout, "configured");
            EXIT_SUCCESS
        }
        Err(_) => keyring_error(stderr),
    }
}

fn read_bounded_input(reader: &mut impl Read) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut input = Zeroizing::new(Vec::with_capacity(MAX_NEWS_TOKEN_BYTES + 1));
    reader
        .take((MAX_NEWS_TOKEN_BYTES + 1) as u64)
        .read_to_end(&mut input)?;
    Ok(input)
}

fn run_status<S: NewsTokenStore, W: Write, E: Write>(
    store: &S,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    match store.load() {
        Ok(Some(_)) => {
            let _ = writeln!(stdout, "configured");
            EXIT_SUCCESS
        }
        Ok(None) => {
            let _ = writeln!(stdout, "not-configured");
            EXIT_NOT_CONFIGURED
        }
        Err(_) => keyring_error(stderr),
    }
}

fn run_delete<S: NewsTokenStore, W: Write, E: Write>(
    store: &S,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    match store.delete() {
        Ok(()) => {
            let _ = writeln!(stdout, "deleted");
            EXIT_SUCCESS
        }
        Err(_) => keyring_error(stderr),
    }
}

fn usage_error(stderr: &mut impl Write) -> i32 {
    let _ = writeln!(stderr, "invalid usage or token");
    EXIT_USAGE_OR_INVALID_TOKEN
}

fn keyring_error(stderr: &mut impl Write) -> i32 {
    let _ = writeln!(stderr, "keyring failure");
    EXIT_KEYRING_FAILURE
}

#[cfg(test)]
#[path = "news_provision_tests.rs"]
mod tests;
