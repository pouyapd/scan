// Main file is only for us to test the library, it will not be used by scan
use std::{io::self, path::PathBuf};
use boa_interner::Interner;
// Import the build function and the Scxml struct from the lib crate
use crate::build;
use crate::build_parser::Scxml; // Assuming Scxml is re-exported via build_parser module in lib.rs

fn main() {
    // Asks the user to enter a directory
    let mut guess = String::new();
    println!("Please enter the path to the SCXML file or directory:"); // Added prompt for user
    io::stdin()
        .read_line(&mut guess)
        .expect("Failed to read line"); // expect is okay here for basic input error handling

    let dir = guess.trim();
    let path = PathBuf::from(dir);
    let mut interner_c = Interner::new();

    // Call the build function which returns Result<Scxml, Error>
    let build_result = build(path, &mut interner_c);

    // Handle the Result:
    match build_result {
        Ok(scxml_instance) => {
            // If build was successful, call the stamp method on the Scxml instance
            println!("SCXML built successfully!");
            scxml_instance.stamp();
        }
        Err(e) => {
            // If build failed, print the error details
            eprintln!("Error building SCXML: {:?}", e);
            // Exit the program with a non-zero status code to indicate failure
            std::process::exit(1);
        }
    }
}