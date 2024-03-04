These two skills are related to two nodes of the behavior tree. 
Battery Drainer is relative to an Action node, while Battery Level is related to a Condition node.
The main differences between them are:
    - The Action skill can return IDLE, SUCCESS, FAILURE, STARTED, STOPPED or RUNNING, while the condition skill can only return IDLE, SUCCESS, STARTED, STOPPED or FAILURE.
    - Once it is started the action skill can terminate after more than one tick, while the condition skill should terminate immediately.
    - After the first tick the action skill remains in the started state (it is not started again unless it has received a stop), while the Condition skill is stopped after each tick.

The behavior of the two skills are:

Battery Drainer:
    - "idle": The state machine begin in an idle state and waits the start command from the behavior tree (initialisation of the state machine). This state return IDLE.
    - "active": Once it is started it waits in an active state for a tick. if the skill is stopped by the behavior tree it goes to the idle state. This state returns STARTED.
    - "drain": At the time of the tick it goes to the drain state and waits for the component to terminates. Once the component has terminated it returns to the state active, otherwise if the skill is stopped by the behavior tree it goes to the idle state. This state returns RUNNING.

Battery Level: 
    - "idle":  The state machine begin in an idle state and waits the start command from the behavior tree. This state return IDLE.
    - "get": This state gets the percentage from the battery. If the value is above 30 it goes to a success state, otherwise it goes to a failure state. This state return STARTED.
    - "success": This state waits for the ok command to return to idle. This state return SUCCESS.
    - "failure": This state waits for the ok command to return to idle. This state return FAILURE.