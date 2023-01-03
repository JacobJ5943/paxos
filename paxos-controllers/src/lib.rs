/// Design for the local controllers
///
/// The idea is to have one [`LocalMessageController`][`local_controller::LocalMessageController`] which will act as the man in the middle.  
/// When the [`LocalMessageController`][`local_controller::LocalMessageController`] is created it will spin up a [`tokio::task`] which will loop infinitely listening for [`local_controller::Messages`]
///
/// Each proposer will be given a [`LocalMessageSender`][`local_controller::LocalMessageSender`] which will send the messages to the LocalMessageController
///
/// Then use [`LocalMessageController.try_send_message`][`local_controller::LocalMessageController::try_send_message`] to send the [Message][`local_controller::Messages`];
pub mod local_controller;
#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
