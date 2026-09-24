<?php
            final class AppInfo {
                public const string DOMAIN = 'example.com';
                public const string URL = 'https://' . self::DOMAIN;
            }
            /** @param array<string> $urls */
            function wantsStringArray(array $urls): void {}
            function f(): void {
                wantsStringArray([
                    'HOME' => AppInfo::URL,
                    'WIKI' => AppInfo::URL . '/doc',
                ]);
            }
        