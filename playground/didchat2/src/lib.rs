pub fn run(config: Config) -> Result<String, std::io::Error> {
    match config.cmd {
        CMD::Init{ path } => init(&path),
        CMD::Doc{ path } => doc(&path),
        CMD::Did{ path } => did(&path),
        CMD::Messages{ path } => messages(&path),
        CMD::Connect{ path, name, did } => connect(&path, &name, &did),
        CMD::Write{ path, name, message } => write(&path, &name, &message),
        CMD::Read{ path, encrypted_message } => read(&path, &encrypted_message),
        CMD::Help => help()
    }
}


fn init(path: &str) -> Result<String, std::io::Error> {
    use std::io::Write;

    // 1. Create empty folders
    if !std::fs::metadata(root_path(path)).is_ok() {
        std::fs::create_dir_all(root_path(path))?;
    }
    if !std::fs::metadata(names_path(path)).is_ok() {
        std::fs::create_dir_all(names_path(path))?;
    }
    if !std::fs::metadata(dids_path(path)).is_ok() {
        std::fs::create_dir_all(dids_path(path))?;
    }
    if !std::fs::metadata(messages_path(path)).is_ok() {
        std::fs::create_dir_all(messages_path(path))?;
    }

    if !std::fs::metadata(seed_path(path)).is_ok() {
        // 2. Generate seed
        let mut csprng = rand_core::OsRng{};
        let seed = ed25519_dalek::SecretKey::generate(&mut csprng);
        let seed_bytes = seed.as_bytes();

        // 3. Write seed to file
        let mut file = std::fs::File::create(seed_path(path)).unwrap();
        file.write(seed_bytes).unwrap();
        
        Ok(format!("{} is ready", path))
    } else {
        Ok(format!("{} already exists", path))
    }
}


fn doc(path: &str) -> Result<String, std::io::Error> {
    use did_key::DIDCore;
    use did_key::KeyMaterial;
    
    // 1. Read seed from file
    let seed = std::fs::read(seed_path(path)).unwrap();

    // 2. Transform seed to a did-document
    let public_private_keypair = did_key::Ed25519KeyPair::from_seed(&seed);
    let public_only_keypair = did_key::from_existing_key::<did_key::Ed25519KeyPair>(&public_private_keypair.public_key_bytes(), None);
    let doc = public_only_keypair.get_did_document(did_key::CONFIG_LD_PUBLIC);

    // 3. Serialize did-document to json
    let did_document = serde_json::to_string_pretty(&doc).unwrap();
    Ok(format!("{}", did_document))
}


fn did(path: &str) -> Result<String, std::io::Error> {
    use did_key::DIDCore;

    // 1. Read seed from file
    let seed = std::fs::read(seed_path(path)).unwrap();

    // 2. Transform seed to a did
    let keypair = did_key::Ed25519KeyPair::from_seed(&seed);
    let diddoc: did_key::Document = keypair.get_did_document(did_key::CONFIG_LD_PUBLIC);
    let did = diddoc.id;

    // 3. Print did
    Ok(format!("{}", did))
}


fn connect(path: &str, name: &str, did: &str) -> Result<String, std::io::Error> {
    use std::io::Write;

    // 2. Create 'name' -> 'did' mapping
    let mut file = std::fs::File::create(name_path(path, name)).unwrap();
    file.write(did.as_bytes()).unwrap();

    // 3. Create 'did' to 'name' mapping
    let mut file = std::fs::File::create(did_path(path, did)).unwrap();
    file.write(name.as_bytes()).unwrap();

    Ok(format!("{}\n{}", name_path(path, name), did_path(path, did)))
}


fn write(path: &str, name: &str, message: &str) -> Result<String, std::io::Error> {
    use did_key::KeyMaterial;
    use std::io::Write;

    // 1. Read from-key
    let from_seed = std::fs::read(seed_path(path)).unwrap();
    let from_key = did_key::Ed25519KeyPair::from_seed(&from_seed);
    
    // 2. Read to-key
    let to_did = std::fs::read_to_string(name_path(path, name)).unwrap();
    let to_key = did_key::resolve(&to_did).unwrap();
    let to_key = did_key::Ed25519KeyPair::from_public_key(&to_key.public_key_bytes());

    // 3. Encrypt message with from_key, to keep message history in local file
    let encrypted_message = encrypt_didcomm(&from_key, &from_key, message).unwrap();
    let mut file = std::fs::File::create(message_path(path)).unwrap();
    file.write(encrypted_message.as_bytes()).unwrap();

    // 4. Encrypt message with to_key, to prepare it for transmission
    let encrypted_message = encrypt_didcomm(&from_key, &to_key, message).unwrap();
    
    Ok(format!("{}", &encrypted_message))
}


fn read(path: &str, encrypted_message: &str) -> Result<String, std::io::Error> {
    use std::io::Write;
    use did_key::KeyMaterial;

    // 1. Store incomming message to file, to keep the message history
    let message_fpath = message_path(path);
    let message_fpath = std::path::Path::new(&message_fpath);
    let mut file = std::fs::File::create(message_fpath).unwrap();
    file.write(encrypted_message.as_bytes()).unwrap();

    // 2. Get to-key
    let to_seed = std::fs::read(seed_path(path)).unwrap();
    let to_key = did_key::Ed25519KeyPair::from_seed(&to_seed);
    
    // 3. Get from-key
    let from_jwe: didcomm_rs::Jwe = serde_json::from_str(&encrypted_message).unwrap();
    let from_did = from_jwe.from().as_ref().unwrap();
    let from_key = did_key::resolve(&from_did).unwrap();
    let from_key = did_key::Ed25519KeyPair::from_public_key(&from_key.public_key_bytes());

    // 4. Decrypt message
    let decrypted = decrypt_didcomm(&from_key, &to_key, encrypted_message).unwrap();

    // 5. Format
    let from_name = std::fs::read_to_string(did_path(path, from_did))
        .unwrap_or(from_did.clone());
    let filename = &message_fpath.file_name().unwrap().to_str().unwrap();
    Ok(format!("[{}] {} > {}", filename, from_name, decrypted))
}


fn messages(path: &str) -> Result<String, std::io::Error> {
    use did_key::KeyMaterial;

    let mut result = String::from("");

    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(messages_path(path)).unwrap().filter_map(|f| f.ok()).collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        if entry.path().is_dir() {
            continue;
        }
        let encrypted_message = std::fs::read_to_string(entry.path())?;

        // 1. Get to-key
        let to_seed = std::fs::read(seed_path(path)).unwrap();
        let to_key = did_key::Ed25519KeyPair::from_seed(&to_seed);

        // 2. Get from-key
        let from_jwe: didcomm_rs::Jwe = serde_json::from_str(&encrypted_message).unwrap();
        let from_did = from_jwe.from().as_ref().unwrap();
        let from_key = did_key::resolve(&from_did).unwrap();
        let from_key = did_key::Ed25519KeyPair::from_public_key(&from_key.public_key_bytes());

        // 3. Decrypt message
        let decrypted = decrypt_didcomm(&from_key, &to_key, &encrypted_message).unwrap();

        // 4. Format
        let from_name = std::fs::read_to_string(did_path(path, from_did))
            .unwrap_or(from_did.clone());
        let file_name = String::from(entry.file_name().to_str().unwrap());
        result.push_str(&format!("[{}] {} > {}\n", file_name, from_name, decrypted));
    }

    Ok(result)
}


fn help() -> Result<String, std::io::Error> {
    Ok(String::from("
    Usage:
        didchat <path> <command>
        
        didchat <path> init
        didchat <path> doc    
        didchat <path> did    
        didchat <path> messages

        didchat <path> connect <name> <did>

        didchat <path> write <name> <message>      -->  <encrypted message>
        didchat <path> read <encrypted message>    -->  <name> <message>

    Example - Write to self:
        didchat . init
        didchat . connect self $(didchat . did)
        didchat . read $(didchat . write self \"Hello self!\")

    Example - Write to peer:
        didchat jonas init
        didchat snorre init
        
        didchat snorre connect jonas $(didchat jonas did)
        didchat jonas connect snorre $(didchat snorre did)

        didchat jonas read $(didchat snorre write jonas \"Hello Jonas. How are you?\")
        didchat snorre read $(didchat jonas write snorre \"Hi Snorre:) I have seen better days.\")
"))
}


#[derive(Debug)]
enum CMD {
    Init{ path: String },
    Doc{ path: String },
    Did{ path: String },
    Messages{ path: String },
    Connect{ path: String, name: String, did: String },
    Write{ path: String, name: String, message: String },
    Read{ path: String, encrypted_message: String },
    Help
}

pub struct Config {
    cmd: CMD,
}

impl Config {
    pub fn new(args: &[String]) -> Result<Config, std::io::Error> {
        let default_cmd = String::from("help");
        
        let path = args.get(1).unwrap_or(&default_cmd).clone();
        let cmd = args.get(2).unwrap_or(&default_cmd).clone();

        let cmd = if args.len() < 3 {
            eprintln!("Command missing!");
            default_cmd.clone()
        } else {
            cmd.clone()
        };

        let cmd: CMD = match &cmd[..] {
            "did" => {
                CMD::Did{ path }
            },
            "doc" => {
                CMD::Doc{ path }
            },
            "init" => {
                CMD::Init{ path }
            },
            "messages" => {
                CMD::Messages{ path }
            },
            "connect" => {
                let name = (match args.get(3) {
                    Some(arg) => arg,
                    None => return Ok(Config{ cmd: CMD::Help }),
                }).clone();

                let did = (match args.get(4) {
                    Some(arg) => arg,
                    None => return Ok(Config{ cmd: CMD::Help }),
                }).clone();
                
                CMD::Connect{ path, name, did }
            },
            "write" => {
                let name = (match args.get(3) {
                    Some(arg) => arg,
                    None => return Ok(Config{ cmd: CMD::Help }),
                }).clone();

                let message = (match args.get(4) {
                    Some(arg) => arg,
                    None => return Ok(Config{ cmd: CMD::Help }),
                }).clone();

                CMD::Write{ path, name, message }
            },
            "read" => {
                let encrypted_message = (match args.get(3) {
                    Some(arg) => arg,
                    None => return Ok(Config{ cmd: CMD::Help }),
                }).clone();

                CMD::Read{ path, encrypted_message }
            },
            "help" => CMD::Help,
            &_ => {
                eprintln!("{} not a valid command!", cmd);
                CMD::Help
            },
        };

        Ok(Config { cmd })
    }
}

fn root_path(path: &str) -> String {
    format!("{}/.didchat", path)
}

fn seed_path(path: &str) -> String {
    format!("{}/.didchat/seed", path)
}

fn names_path(path: &str) -> String {
    format!("{}/.didchat/names", path)
}

fn dids_path(path: &str) -> String {
    format!("{}/.didchat/dids", path)
}

fn messages_path(path: &str) -> String {
    format!("{}/.didchat/messages", path)
}

fn message_path(path: &str) -> String {
    let start = std::time::SystemTime::now();
    let since_the_epoch = start
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards");

    format!("{}/.didchat/messages/{}.dcem", path, since_the_epoch.as_nanos())
}

fn name_path(path: &str, name: &str) -> String {
    format!("{}/.didchat/names/{}", path, name)
}

fn did_path(path: &str, did: &str) -> String {
    format!("{}/.didchat/dids/{}", path, did)
}

fn encrypt_didcomm(from_key: &did_key::Ed25519KeyPair, to_key: &did_key::Ed25519KeyPair, message: &str) -> Result<String, didcomm_rs::Error> {
    use did_key::Ecdh;
    use did_key::DIDCore;

    // 1. Get dids
    let from_did = from_key.get_did_document(did_key::CONFIG_LD_PUBLIC).id;
    let to_did = to_key.get_did_document(did_key::CONFIG_LD_PUBLIC).id;

    // 2. Map Ed25519 -> x25519
    let from_key = from_key.get_x25519();
    let to_key = to_key.get_x25519();

    // 3. Make shared secret (from -> to)
    let shared_secret = from_key.key_exchange(&to_key);

    // 4. Make didcomm message
    let to_vec = vec![&to_did[..]];
    
    let message = didcomm_rs::Message::new()
        .from(&from_did)
        .to(&to_vec[..])
        .timed(Some(3600))
        .body(message.as_bytes())
        .as_jwe(&didcomm_rs::crypto::CryptoAlgorithm::XC20P);

    // 5. Seal message using shared secret
    let encrypted_message = message
        .seal(&shared_secret)
        .unwrap();

    Ok(encrypted_message)
}

fn decrypt_didcomm(from_key: &did_key::Ed25519KeyPair, to_key: &did_key::Ed25519KeyPair, encrypted_message: &str)-> Result<String, didcomm_rs::Error> {
    use did_key::Ecdh;

    // 1. Map Ed25519 -> x25519
    let to_key = to_key.get_x25519();
    let from_key = from_key.get_x25519();

    // 2. Make shared secret (to -> from)
    let shared_secret = to_key.key_exchange(&from_key);

    // 3. Decrypt message
    let decrypted = didcomm_rs::Message::receive(encrypted_message, Some(&shared_secret), None);
    let decrypted = decrypted.unwrap();
    let decrypted = String::from_utf8(decrypted.body).unwrap();

    Ok(decrypted)
}