use ahash::{AHashMap, AHashSet};

#[derive(Default)]
pub struct OptimizedRoutingTable {
    topic_subscribers: AHashMap<u32, AHashSet<u32>>,
    entity_subscriptions: AHashMap<u32, AHashSet<u32>>,
}

impl OptimizedRoutingTable {
    pub fn subscribe(&mut self, entity_id: u32, topic: u32) {
        self.topic_subscribers.entry(topic).or_default().insert(entity_id);
        self.entity_subscriptions.entry(entity_id).or_default().insert(topic);
    }

    pub fn unsubscribe(&mut self, entity_id: u32, topic: u32) {
        if let Some(subs) = self.topic_subscribers.get_mut(&topic) {
            subs.remove(&entity_id);
            if subs.is_empty() { self.topic_subscribers.remove(&topic); }
        }
        if let Some(topics) = self.entity_subscriptions.get_mut(&entity_id) {
            topics.remove(&topic);
            if topics.is_empty() { self.entity_subscriptions.remove(&entity_id); }
        }
    }

    pub fn unsubscribe_all(&mut self, entity_id: u32) {
        if let Some(topics) = self.entity_subscriptions.remove(&entity_id) {
            for topic in topics {
                if let Some(subs) = self.topic_subscribers.get_mut(&topic) {
                    subs.remove(&entity_id);
                    if subs.is_empty() { self.topic_subscribers.remove(&topic); }
                }
            }
        }
    }

    pub fn get_subscribers(&self, topic: &u32) -> Option<&AHashSet<u32>> {
        self.topic_subscribers.get(topic)
    }

    pub fn remove_topic(&mut self, topic: u32) -> Option<AHashSet<u32>> {

        let subscribers = self.topic_subscribers.remove(&topic)?;

        for entity_id in &subscribers {
            if let Some(topics) = self.entity_subscriptions.get_mut(entity_id) {
                topics.remove(&topic);
                if topics.is_empty() {
                    self.entity_subscriptions.remove(entity_id);
                }
            }
        }

        Some(subscribers)
    }
}