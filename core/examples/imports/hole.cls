namespace examples.imports.ant

// Imports whole schemas by name from a underlying namespace
import examples::imports::{ant food}

// Or imports exactly the components you want like structures, enums and errors
import examples::imports::ant {Ant, Mood, CantCarryError}
import examples::imports::food {Food, State, NotEdibleError}

settings {
}

protocol Gathering {

}