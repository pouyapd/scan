 //Main file is only for us to test the library, it will not be used by scan
use std::{io::self, path::PathBuf};
use boa_interner::Interner;
use scxml_lib::build;

//The main of the project takes as input a folder containing the files.
//Checks if the file has the scxm extension (if not, it generates a warning and does not consider that file)
//Each file is read line by line and a tree is created from it.
//Finally, it prints the path of each file with the respective tree
fn main() {
    //Asks the user to enter a directory
    let mut guess= String::new();
    io::stdin()
        .read_line(&mut guess)
        .expect("Failed to read line");
    let dir = guess.trim();    
    //Use the build function of the lib.rs file and print the result to the screen
    //let test = build(dir.to_string());
    let path = PathBuf::from(dir);
    let mut interner_c= Interner::new();
    let test = build(path,&mut interner_c);
    test.stamp();
}

