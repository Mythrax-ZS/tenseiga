-- The Twin Flames Saga's quests (rAthena db/custom/samurai/quest_db.yml), for the quest log.
-- Same layout as System\OngoingQuestInfoList_True.lua.
QuestInfoList = {}
QuestInfoList[900101] = {
    Title = "Twin Flames I: Ashes of Honor",
    Summary = "Seek Oharu in Amatsu, and pray at the forgotten shrine.",
    Description = { "Kagemori, the village above Amatsu where the house you served stood, has been burned by raiders.", "Old Oharu, the only survivor, waits by the town guides.", "She says to pray at the forgotten shrine in the fields north of town." },
    NpcNavi = "amatsu",
    NpcPosX = 192,
    NpcPosY = 94,
}
QuestInfoList[900103] = {
    Title = "Twin Flames I: Kagemori Burning",
    Summary = "Walk through the night Kagemori burned.",
    Description = { "A hooded kami at the forgotten shrine answered your prayer with a vision of that night.", "Free the villagers trapped in the flames, find Oharu and the lord, and face the Torch Captain.", "Speak to the kami again to go back into the vision." },
    NpcNavi = "ama_in01",
    NpcPosX = 175,
    NpcPosY = 176,
}
QuestInfoList[900102] = {
    Title = "Twin Flames I: Ash on the Wind",
    Summary = "Hunt 20 Ashigaru Raiders in the Amatsu fields.",
    Description = { "A kami of justice answered your prayer with a blade: Muramasa.", "Hunt the Ashigaru Raiders in the fields north of Amatsu, then return to Oharu." },
    NpcNavi = "ama_fild01",
    NpcPosX = 0,
    NpcPosY = 0,
}
QuestInfoList[900111] = {
    Title = "Twin Flames II: Yokai of the River",
    Summary = "Drive back 30 Kapha for Elder Genzo.",
    Description = { "The Kapha of the river have grown bold.", "Hunt 30 of them in the Amatsu fields, then return to Elder Genzo." },
    NpcNavi = "ama_fild01",
    NpcPosX = 0,
    NpcPosY = 0,
}
QuestInfoList[900112] = {
    Title = "Twin Flames II: The Captured Raider",
    Summary = "Decide the captured raider's fate.",
    Description = { "One of the raiders who burned your lord's house has been caught.", "He kneels bound beside Elder Genzo in Amatsu, under guard. His fate is yours." },
    NpcNavi = "amatsu",
    NpcPosX = 187,
    NpcPosY = 158,
}
QuestInfoList[900121] = {
    Title = "Twin Flames III: The Trickster's Tail",
    Summary = "Answer Tetsuya's riddle.",
    Description = { "A man with a monkey's tail has been watching you: Tetsuya Takahashi.", "Answer his riddle in Amatsu." },
    NpcNavi = "amatsu",
    NpcPosX = 192,
    NpcPosY = 163,
}
QuestInfoList[900122] = {
    Title = "Twin Flames III: Kurogane's Fortress",
    Summary = "Storm Warlord Kurogane's fortress.",
    Description = { "Warlord Kurogane paid the raiders.", "Go through the iron gate on the second floor of the Amatsu dungeon, with Muramasa in hand. Tetsuya waits on the walls.", "Cut down his three iron banners, face him in his hall, then return to Tetsuya." },
    NpcNavi = "ama_dun02",
    NpcPosX = 45,
    NpcPosY = 50,
}
QuestInfoList[900131] = {
    Title = "Twin Flames IV: The Crimson Fields",
    Summary = "Walk the Crimson Fields as they were when the armies met.",
    Description = { "Kurogane's death broke the truce, and the clan armies tore the fields north of Amatsu apart.", "Touch the scorched war banner past the stone path. Cut down 30 soldiers, but spare the villagers cowering among them.", "Then face what waits beneath the banner." },
    NpcNavi = "ama_fild01",
    NpcPosX = 200,
    NpcPosY = 291,
}
QuestInfoList[900132] = {
    Title = "Twin Flames IV: The Disgraced General",
    Summary = "Defeat Usurper General Mibu for Kiyoshi Takeda.",
    Description = { "General Mibu betrayed Kiyoshi Takeda to Yami's cult.", "Tear down his war banner on the second floor of the Ice Dungeon, then return to Kiyoshi in Amatsu." },
    NpcNavi = "ice_dun02",
    NpcPosX = 150,
    NpcPosY = 155,
}
QuestInfoList[900133] = {
    Title = "Twin Flames IV: The Fallen Monk",
    Summary = "Grant Fallen Monk Genkai rest for Brother Renga.",
    Description = { "Genkai fell to Yami's whispers.", "Find his desecrated altar in the fields outside St. Capitolina Abbey, then return to Brother Renga in Amatsu." },
    NpcNavi = "prt_monk",
    NpcPosX = 225,
    NpcPosY = 140,
}
QuestInfoList[900134] = {
    Title = "Twin Flames IV: The Hidden Leaf",
    Summary = "Defeat Kage, who slaughtered Ayame's clan.",
    Description = { "Kage led the slaughter of Ayame's clan.", "Pull his kunai from the wall on the second floor of the Amatsu dungeon.", "Bring Ayame her clan's kunai, and 5 Holy Water to purify it." },
    NpcNavi = "ama_dun02",
    NpcPosX = 40,
    NpcPosY = 50,
}
QuestInfoList[900141] = {
    Title = "Twin Flames V: The Shrine of Seals",
    Summary = "Open the Sarugami's seals with Tetsuya.",
    Description = { "Something sealed beneath Amatsu wants your cursed blade and Tetsuya's blood.", "Go through the Sealed Gate on the deepest floor of the Amatsu dungeon, with Muramasa in hand.", "Open the seals of the Curse, the Blood and the Truth, and face their guardian." },
    NpcNavi = "ama_dun03",
    NpcPosX = 120,
    NpcPosY = 30,
}
QuestInfoList[900142] = {
    Title = "Twin Flames V: The Shrine Maiden",
    Summary = "Stand with Chiyo against High Priestess Sayo.",
    Description = { "Sayo came to take Kusanagi for the goddess, and her pupil Chiyo has turned against her.", "Defeat Sayo in the shrine's high hall; her acolytes keep her shielded.", "Go back through the Sealed Gate if you left the shrine." },
    NpcNavi = "ama_dun03",
    NpcPosX = 120,
    NpcPosY = 30,
}
QuestInfoList[900151] = {
    Title = "Twin Flames VI: The Twin Exorcists",
    Summary = "Break the frozen seal for Taro and Jiro.",
    Description = { "A yokai sealed itself in a glacier, and the twin exorcists cannot agree how to seal it again.", "Break the frozen seal on the third floor of the Ice Dungeon. When their ritual falters, talk them round.", "Then return to Taro and Jiro in Amatsu." },
    NpcNavi = "ice_dun03",
    NpcPosX = 149,
    NpcPosY = 45,
}
QuestInfoList[900152] = {
    Title = "Twin Flames VI: The Palace of Masks",
    Summary = "Find Lady Hanae in the Amatsu palace.",
    Description = { "Lady Hanae, an exile from court, could use a samurai's help.", "Find her inside the Amatsu palace, beside its lord." },
    NpcNavi = "ama_in02",
    NpcPosX = 196,
    NpcPosY = 172,
}
QuestInfoList[900153] = {
    Title = "Twin Flames VI: The Palace of Masks",
    Summary = "Unmask Daimyo Kurozane at his masquerade.",
    Description = { "Daimyo Kurozane made a pact with Yami's cult and had Lady Hanae's father exiled for learning of it.", "Go into the masquerade with Lady Hanae. Find the cult's three sealed letters among the guest wing's chests.", "Each letter rules out one of the four masked stewards of the inner court. Accuse the one who is left." },
    NpcNavi = "ama_in02",
    NpcPosX = 196,
    NpcPosY = 172,
}
QuestInfoList[900161] = {
    Title = "Twin Flames VII: Return of the Ashes",
    Summary = "Defeat 30 Cultists of Ashes and the Herald of Ashes.",
    Description = { "Yami's cult marches openly.", "Hunt 30 Cultists of Ashes on Thor's volcano, then face the Herald at the Altar of Ashes on its second level.", "Only the Twin Flames can call him: Kusanagi in one hand, Muramasa in the other, and both will pull at you." },
    NpcNavi = "thor_v02",
    NpcPosX = 90,
    NpcPosY = 199,
}
QuestInfoList[900162] = {
    Title = "Twin Flames VII: The Blacksmith's Son",
    Summary = "Bring Masaru 5 pages of his ancestor's ledger.",
    Description = { "Masaru's family forged Muramasa.", "The Kasa and Salamanders of Thor's volcano swallowed the pages of the smith's ledger.", "Bring 5 to Masaru in Prontera." },
    NpcNavi = "prontera",
    NpcPosX = 161,
    NpcPosY = 179,
}
QuestInfoList[900171] = {
    Title = "Twin Flames VIII: The Fox and the Contract",
    Summary = "Burn Kohaku's contract.",
    Description = { "A white spirit fox has been watching you for the goddess.", "Find Kohaku in the woods east of Payon and defeat the Contract Binder." },
    NpcNavi = "pay_fild08",
    NpcPosX = 40,
    NpcPosY = 80,
}
QuestInfoList[900172] = {
    Title = "Twin Flames VIII: The Fall of Yami",
    Summary = "Go down into the Abyssal Pit with Kohaku.",
    Description = { "Kagutsuchi-no-Yami waits at the bottom of Thor's volcano.", "Go through the Abyssal Gate, break the four Chain Anchors on the way down, and face him at the heart of the pit." },
    NpcNavi = "thor_v03",
    NpcPosX = 49,
    NpcPosY = 250,
}
QuestInfoList[900181] = {
    Title = "Twin Flames IX: The Weaver Unmasked",
    Summary = "Gather every relic, then face the Sarugami's trial with Tetsuya.",
    Description = { "The goddess's blessing has cracked, and Yami's fall was her trap.", "Bring Tetsuya in Amatsu all eight relics: Ayame's kunai, Renga's beads, Kiyoshi's crest, Chiyo's talisman,", "the twins' sigils, Hanae's jade, Masaru's ember and Kohaku's foxfire. Then go with him to the Sarugami's trial ground." },
    NpcNavi = "amatsu",
    NpcPosX = 192,
    NpcPosY = 163,
}
QuestInfoList[900191] = {
    Title = "Twin Flames X: The First Forge",
    Summary = "Forge Tenseiga-no-Hayate at the First Forge.",
    Description = { "Masaru waits at the First Forge, on the second level of the Magma Dungeon.", "Bring both blades in hand, every relic, 10 Oridecon and 10 Burning Hearts." },
    NpcNavi = "mag_dun02",
    NpcPosX = 49,
    NpcPosY = 40,
}
QuestInfoList[900192] = {
    Title = "Twin Flames X: The Celestial Veil",
    Summary = "Cut the Weaver's chains.",
    Description = { "Amaterasu-no-Mikazuki waits in the Celestial Veil. Tetsuya will take you there from the First Forge.", "Break the echoes she wears on the outer platforms, then face her at the throne with Tenseiga-no-Hayate.", "When the sun gathers, stand by a Stone of Shade." },
    NpcNavi = "mag_dun02",
    NpcPosX = 53,
    NpcPosY = 44,
}
