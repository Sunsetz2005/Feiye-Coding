import {
  ConversationThread,
  type ConversationThreadProps,
} from "@/components/lobe-chat";
import { useViewedMessages } from "@/entities/session";

/**
 * Subscribes to the transcript store so token patches do not re-render App.
 */
export function ConversationSurface(
  props: Omit<ConversationThreadProps, "messages">,
) {
  const messages = useViewedMessages();
  return <ConversationThread {...props} messages={messages} />;
}
