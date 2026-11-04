/// Event bus for decoupling Mopidy client from WebSocket handlers
/// Uses pub/sub pattern to allow loose coupling between components
import gleam/erlang/process.{type Subject}
import gleam/list
import gleam/otp/actor
import logging

/// Events published by the Mopidy client
pub type MopidyEvent {
  /// Mopidy connection established
  Connected
  /// Mopidy connection lost
  Disconnected
  /// Message received from Mopidy (JSON-RPC response or event)
  MessageReceived(data: String)
  /// Error occurred
  Error(error: String)
}

/// Commands that can be sent to Mopidy
pub type MopidyCommand {
  /// Send a JSON-RPC message to Mopidy
  SendMessage(data: String)
}

/// Internal messages for the event bus actor
pub type BusMessage {
  /// Subscribe to Mopidy events
  Subscribe(subscriber: Subject(MopidyEvent))
  /// Unsubscribe from Mopidy events
  Unsubscribe(subscriber: Subject(MopidyEvent))
  /// Publish a Mopidy event to all subscribers
  Publish(event: MopidyEvent)
  /// Register the Mopidy command handler
  RegisterCommandHandler(handler: Subject(MopidyCommand))
  /// Send a command to Mopidy (routed to command handler)
  SendCommand(command: MopidyCommand)
}

/// State for the event bus actor
pub opaque type State {
  State(
    subscribers: List(Subject(MopidyEvent)),
    command_handler: option.Option(Subject(MopidyCommand)),
  )
}

import gleam/option

/// Start the event bus actor
pub fn start() -> Result(Subject(BusMessage), actor.StartError) {
  let initial_state = State(subscribers: [], command_handler: option.None)

  actor.new(initial_state)
  |> actor.on_message(handle_message)
  |> actor.start
  |> result.map(fn(started) { started.data })
}

import gleam/result

/// Handle incoming messages
fn handle_message(
  state: State,
  message: BusMessage,
) -> actor.Next(State, BusMessage) {
  case message {
    Subscribe(subscriber) -> {
      logging.log(logging.Debug, "Event bus: new subscriber registered")
      let new_subscribers = [subscriber, ..state.subscribers]
      actor.continue(State(..state, subscribers: new_subscribers))
    }

    Unsubscribe(subscriber) -> {
      logging.log(logging.Debug, "Event bus: subscriber removed")
      let new_subscribers =
        list.filter(state.subscribers, fn(s) { s != subscriber })
      actor.continue(State(..state, subscribers: new_subscribers))
    }

    Publish(event) -> {
      // Broadcast event to all subscribers
      list.each(state.subscribers, fn(subscriber) {
        process.send(subscriber, event)
      })
      actor.continue(state)
    }

    RegisterCommandHandler(handler) -> {
      logging.log(logging.Debug, "Event bus: command handler registered")
      actor.continue(State(..state, command_handler: option.Some(handler)))
    }

    SendCommand(command) -> {
      case state.command_handler {
        option.Some(handler) -> {
          process.send(handler, command)
        }
        option.None -> {
          logging.log(
            logging.Warning,
            "Event bus: no command handler registered, dropping command",
          )
        }
      }
      actor.continue(state)
    }
  }
}

// Convenience functions for interacting with the event bus

/// Subscribe to Mopidy events
pub fn subscribe(
  bus: Subject(BusMessage),
  subscriber: Subject(MopidyEvent),
) -> Nil {
  process.send(bus, Subscribe(subscriber))
}

/// Unsubscribe from Mopidy events
pub fn unsubscribe(
  bus: Subject(BusMessage),
  subscriber: Subject(MopidyEvent),
) -> Nil {
  process.send(bus, Unsubscribe(subscriber))
}

/// Publish a Mopidy event
pub fn publish(bus: Subject(BusMessage), event: MopidyEvent) -> Nil {
  process.send(bus, Publish(event))
}

/// Register the Mopidy command handler
pub fn register_command_handler(
  bus: Subject(BusMessage),
  handler: Subject(MopidyCommand),
) -> Nil {
  process.send(bus, RegisterCommandHandler(handler))
}

/// Send a command to Mopidy
pub fn send_command(bus: Subject(BusMessage), command: MopidyCommand) -> Nil {
  process.send(bus, SendCommand(command))
}
