<?php
        class OldSchool implements Serializable {
            public function serialize() { return ''; }
            public function unserialize($data) {}
        }
        class Modern implements Serializable {
            public function serialize() { return ''; }
            public function unserialize($data) {}
            public function __serialize(): array { return []; }
            public function __unserialize(array $data): void {}
        }
        class ModernChild extends Modern {}
        abstract class AbstractOld implements Serializable {}
    